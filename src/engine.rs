use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::thread;
use std::time::{Duration, Instant};

use midir::MidiOutputConnection;
use midly::MidiMessage;
use midly::MidiMessage::NoteOff;
use midly::live::LiveEvent;
use midly::num::u7;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::MissedTickBehavior;

use crate::common::Time;
use crate::track::{ChannelId, MIDI_CC_SUSTAIN_ID};

pub const MIDI_CHANNEL: ChannelId = 1;

/** Event that is produced by engine. */
#[derive(Clone, Debug)]
pub enum StatusEvent {
    Time(Time),
}

/// A sound event to be rendered by the engine at given time.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct EngineEvent {
    pub at: Time,
    pub event: LiveEvent<'static>,
}

pub type StatusEventReceiver = dyn Fn(StatusEvent) -> () + Send;

impl Ord for EngineEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Ideally the event should also be compared to make the comparison unambiguous.
        // This should not matter for scheduling though.
        other.at.cmp(&self.at)
    }
}

impl PartialOrd for EngineEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub trait EventSource {
    /** Return false when no events will be produced from the source anymore.
    Use this to detach it from the engine. */
    fn is_running(&self) -> bool;
    /** Reset current source's time to this moment. */
    fn seek(&mut self, at: &Time);
    /** The next event to be played at the instant. On subsequent
    calls instants must not decrease unless a reset call sets back the time. */
    fn next(&mut self, at: &Time) -> Vec<EngineEvent>;
}

type EventSourceHandle = dyn EventSource + Send;

pub type EngineCommand = dyn FnOnce(&mut Engine) + Send;

/// Cloneable sender for commands to the engine. `send` is not async, so it can be
/// called from any (synchronous) thread.
pub type EngineHandle = UnboundedSender<Box<EngineCommand>>;

/// How far ahead of the transport clock sources are polled (microseconds).
/// Also bounds how late an edit of a track becomes audible.
const LOOKAHEAD_US: Time = 20_000;
/// Period of status (transport time) notifications.
const STATUS_PERIOD: Duration = Duration::from_millis(20);

pub struct Engine {
    midi_output: MidiOutputConnection,
    sources: Vec<Box<EventSourceHandle>>,
    running_at: Time,
    reset_at: Instant,
    paused: bool,
    status_receiver: Option<Box<StatusEventReceiver>>,
    current_sustain: Option<LiveEvent<'static>>,
    queue: BinaryHeap<EngineEvent>,
    /// Wall-clock scheduled events (audition); independent of transport and pause state.
    timed: Vec<(Instant, LiveEvent<'static>)>,
}

impl Engine {
    pub fn new(midi_output: MidiOutputConnection) -> Engine {
        Engine {
            midi_output,
            sources: Vec::new(),
            running_at: 0,
            reset_at: Instant::now(),
            paused: false,
            status_receiver: None,
            current_sustain: None,
            queue: BinaryHeap::new(),
            timed: Vec::new(),
        }
    }

    /// Run the engine on its own thread (single-threaded tokio runtime).
    /// Commands are sent through the returned handle.
    pub fn start(self) -> EngineHandle {
        let (sender, receiver) = mpsc::unbounded_channel();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("engine runtime");
            runtime.block_on(self.run(receiver));
        });
        sender
    }

    async fn run(mut self, mut commands: UnboundedReceiver<Box<EngineCommand>>) {
        self.seek(0);
        let mut status_tick = tokio::time::interval(STATUS_PERIOD);
        status_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            let deadline = self.next_deadline();
            let timed_at = self.timed.iter().map(|(at, _)| *at).min();
            let timed_deadline = tokio::time::Instant::from_std(timed_at.unwrap_or_else(Instant::now));
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => command(&mut self),
                    None => break, // All handles dropped.
                },
                _ = tokio::time::sleep_until(timed_deadline), if timed_at.is_some() => self.flush_timed(),
                _ = tokio::time::sleep_until(deadline), if !self.paused => self.tick(),
                _ = status_tick.tick(), if !self.paused => {
                    self.advance_clock();
                    self.notify_time();
                }
            }
        }
    }

    /// When the engine should wake up next to play events or poll sources.
    fn next_deadline(&self) -> tokio::time::Instant {
        let poll_at = Instant::now() + Duration::from_micros(LOOKAHEAD_US as u64 / 2);
        let at = match self.queue.peek() {
            Some(ev) => poll_at.min(self.real_time_of(ev.at)),
            None => poll_at,
        };
        tokio::time::Instant::from_std(at)
    }

    fn real_time_of(&self, at: Time) -> Instant {
        self.reset_at + Duration::from_micros(at.max(0) as u64)
    }

    /// Poll sources (with look-ahead) and send out events that are due.
    fn tick(&mut self) {
        self.sources.retain(|s| s.is_running());
        self.advance_clock();
        let horizon = self.running_at + LOOKAHEAD_US;
        let new_events: Vec<EngineEvent> = self
            .sources
            .iter_mut()
            .flat_map(|s| s.next(&horizon))
            .collect();
        self.queue.extend(new_events);
        let mut batch = vec![];
        while let Some(ev) = self.queue.peek() {
            if ev.at > self.running_at {
                break;
            }
            batch.push(self.queue.pop().unwrap().event);
        }
        for ev in batch {
            // Keeping actual value to resume playback with sustain enabled if necessary.
            // Otherwise, it will only be active after next explicit change.
            if let LiveEvent::Midi {
                message: MidiMessage::Controller { controller, .. },
                ..
            } = ev
            {
                if controller == MIDI_CC_SUSTAIN_ID {
                    self.current_sustain = Some(ev.to_static());
                }
            }
            self.process(ev);
        }
    }

    fn advance_clock(&mut self) {
        self.running_at = Instant::now().duration_since(self.reset_at).as_micros() as Time;
    }

    fn notify_time(&mut self) {
        let at = self.running_at;
        self.status_receiver
            .as_mut()
            .map(|recv| recv(StatusEvent::Time(at)));
    }

    pub fn seek(&mut self, at: Time) {
        for s in self.sources.iter_mut() {
            s.seek(&at);
        }
        // Events queued ahead of the old position are stale now.
        self.silence();
        self.running_at = at;
        self.update_realtime();
        self.notify_time();
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if !self.paused {
            self.update_realtime();
        }
        self.process_paused_state();
    }

    pub fn stop(&mut self) {
        self.paused = true;
        self.process_paused_state();
    }

    fn process_paused_state(&mut self) {
        if self.paused {
            self.silence();
        } else if let Some(sustain) = self.current_sustain {
            // TODO (playback) Lookup sustain state from the track at current
            //      time position instead. Would this require cloent-side logic?
            self.queue.push(EngineEvent {
                at: self.running_at,
                event: sustain,
            });
        }
    }

    /// Drop scheduled events and mute ongoing notes.
    fn silence(&mut self) {
        self.queue.clear();
        for key in 0..u7::max_value().into() {
            self.process(LiveEvent::Midi {
                channel: MIDI_CHANNEL.into(),
                message: NoteOff {
                    key: key.into(),
                    vel: 64.into(),
                },
            });
        }
        self.process(LiveEvent::Midi {
            channel: MIDI_CHANNEL.into(),
            message: MidiMessage::Controller {
                controller: MIDI_CC_SUSTAIN_ID.into(),
                value: 0.into(),
            },
        });
    }

    /// Stop all sounds.
    pub fn reset(&mut self) {
        self.paused = true;
    }

    pub fn update_realtime(&mut self) {
        self.reset_at = Instant::now() - Duration::from_micros(self.running_at as u64);
    }

    pub fn add(&mut self, source: Box<EventSourceHandle>) {
        self.sources.push(source);
    }

    /// Play the event after the delay, regardless of transport position or pause state.
    pub fn play_after(&mut self, delay: Duration, event: LiveEvent<'static>) {
        self.timed.push((Instant::now() + delay, event));
    }

    fn flush_timed(&mut self) {
        let now = Instant::now();
        let (due, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut self.timed)
            .into_iter()
            .partition(|(at, _)| *at <= now);
        self.timed = pending;
        for (_, ev) in due {
            self.process(ev);
        }
    }

    /// Process the event immediately.
    pub fn process(&mut self, event: LiveEvent) {
        let mut midi_buf = vec![];
        event.write(&mut midi_buf).unwrap();
        self.midi_output
            .send(&midi_buf)
            .expect("send output MIDI event");
    }

    pub fn set_status_receiver(&mut self, receiver: Option<Box<StatusEventReceiver>>) {
        self.status_receiver = receiver;
    }
}
