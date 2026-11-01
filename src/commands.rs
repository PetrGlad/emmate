use eframe::egui::{Context, Key, KeyboardShortcut, Modifiers};

/// All user-invokable actions. Single source of titles and keyboard shortcuts
/// for shortcut dispatch and the command palette.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Command {
    CommandPalette,
    // App
    Pause,
    Export,
    FollowPlayback,
    ScrollPageLeft,
    ScrollPageRight,
    // Stave: selection and view
    ClearNoteSelection,
    ZoomToFit,
    // Stave: edits
    TempoUp,
    TempoDown,
    DeleteSelection,
    InsertTape,
    ShiftTailLater,
    ShiftTailEarlier,
    ShiftNotesLater,
    ShiftNotesEarlier,
    StretchNotesLonger,
    StretchNotesShorter,
    TransposeUp,
    TransposeDown,
    AccentUp,
    AccentDown,
    Undo,
    Redo,
    Copy,
    Paste,
    SetBookmark,
    ClearBookmark,
    // Stave: cursor navigation
    PrevBookmark,
    NextBookmark,
    PrevEvent,
    NextEvent,
    CursorStart,
    CursorEnd,
}

/// In the order they are listed in the palette when nothing is typed.
pub const ALL: &[Command] = &[
    Command::CommandPalette,
    Command::Pause,
    Command::Export,
    Command::FollowPlayback,
    Command::ScrollPageLeft,
    Command::ScrollPageRight,
    Command::ClearNoteSelection,
    Command::ZoomToFit,
    Command::TempoUp,
    Command::TempoDown,
    Command::DeleteSelection,
    Command::InsertTape,
    Command::ShiftTailLater,
    Command::ShiftTailEarlier,
    Command::ShiftNotesLater,
    Command::ShiftNotesEarlier,
    Command::StretchNotesLonger,
    Command::StretchNotesShorter,
    Command::TransposeUp,
    Command::TransposeDown,
    Command::AccentUp,
    Command::AccentDown,
    Command::Undo,
    Command::Redo,
    Command::Copy,
    Command::Paste,
    Command::SetBookmark,
    Command::ClearBookmark,
    Command::PrevBookmark,
    Command::NextBookmark,
    Command::PrevEvent,
    Command::NextEvent,
    Command::CursorStart,
    Command::CursorEnd,
];

fn plain(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::NONE, key)
}

fn with(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(modifiers, key)
}

impl Command {
    pub fn title(self) -> &'static str {
        match self {
            Command::CommandPalette => "Command palette",
            Command::Pause => "Play/pause",
            Command::Export => "Export to MIDI file",
            Command::FollowPlayback => "Toggle follow playback",
            Command::ScrollPageLeft => "Scroll page left",
            Command::ScrollPageRight => "Scroll page right",
            Command::ClearNoteSelection => "Clear note selection",
            Command::ZoomToFit => "Zoom to fit",
            Command::TempoUp => "Tempo: slow down selection",
            Command::TempoDown => "Tempo: speed up selection",
            Command::DeleteSelection => "Delete selection",
            Command::InsertTape => "Insert tape at time selection",
            Command::ShiftTailLater => "Shift track tail later",
            Command::ShiftTailEarlier => "Shift track tail earlier",
            Command::ShiftNotesLater => "Move selected notes later",
            Command::ShiftNotesEarlier => "Move selected notes earlier",
            Command::StretchNotesLonger => "Make selected notes longer",
            Command::StretchNotesShorter => "Make selected notes shorter",
            Command::TransposeUp => "Transpose selected notes up",
            Command::TransposeDown => "Transpose selected notes down",
            Command::AccentUp => "Increase accent of selected notes",
            Command::AccentDown => "Decrease accent of selected notes",
            Command::Undo => "Undo",
            Command::Redo => "Redo",
            Command::Copy => "Copy time selection",
            Command::Paste => "Paste at cursor",
            Command::SetBookmark => "Set bookmark at cursor",
            Command::ClearBookmark => "Clear bookmark at cursor",
            Command::PrevBookmark => "Go to previous bookmark",
            Command::NextBookmark => "Go to next bookmark",
            Command::PrevEvent => "Go to previous event",
            Command::NextEvent => "Go to next event",
            Command::CursorStart => "Go to track start",
            Command::CursorEnd => "Go to track end",
        }
    }

    /// Key combinations that trigger the command, primary first.
    /// Copy and Paste are delivered by egui as events, not keys, so they have none here.
    pub fn shortcuts(self) -> Vec<KeyboardShortcut> {
        match self {
            Command::CommandPalette => vec![with(Modifiers::CTRL | Modifiers::SHIFT, Key::P)],
            Command::Pause => vec![plain(Key::Space)],
            Command::Export => vec![with(Modifiers::CTRL, Key::S)],
            Command::FollowPlayback => vec![plain(Key::F)],
            Command::ScrollPageLeft => vec![plain(Key::PageUp)],
            Command::ScrollPageRight => vec![plain(Key::PageDown)],
            Command::ClearNoteSelection => vec![plain(Key::Q)],
            Command::ZoomToFit => vec![with(Modifiers::ALT, Key::A)],
            Command::TempoUp => vec![plain(Key::CloseBracket)],
            Command::TempoDown => vec![plain(Key::OpenBracket)],
            Command::DeleteSelection => vec![plain(Key::Delete)],
            Command::InsertTape => vec![plain(Key::Insert)],
            Command::ShiftTailLater => {
                vec![with(Modifiers::CTRL | Modifiers::SHIFT, Key::ArrowRight)]
            }
            Command::ShiftTailEarlier => {
                vec![with(Modifiers::CTRL | Modifiers::SHIFT, Key::ArrowLeft)]
            }
            Command::ShiftNotesLater => vec![
                with(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowRight),
                with(Modifiers::SHIFT, Key::L),
            ],
            Command::ShiftNotesEarlier => vec![
                with(Modifiers::ALT | Modifiers::SHIFT, Key::ArrowLeft),
                with(Modifiers::SHIFT, Key::H),
            ],
            Command::StretchNotesLonger => vec![plain(Key::L)],
            Command::StretchNotesShorter => vec![plain(Key::H)],
            Command::TransposeUp => vec![plain(Key::U)],
            Command::TransposeDown => vec![plain(Key::J)],
            Command::AccentUp => vec![plain(Key::I)],
            Command::AccentDown => vec![plain(Key::K)],
            Command::Undo => vec![with(Modifiers::CTRL, Key::Z)],
            Command::Redo => vec![
                with(Modifiers::CTRL, Key::Y),
                with(Modifiers::CTRL | Modifiers::SHIFT, Key::Z),
            ],
            Command::Copy | Command::Paste => vec![],
            Command::SetBookmark => vec![plain(Key::M)],
            Command::ClearBookmark => vec![plain(Key::N)],
            Command::PrevBookmark => vec![with(Modifiers::CTRL, Key::ArrowLeft)],
            Command::NextBookmark => vec![with(Modifiers::CTRL, Key::ArrowRight)],
            Command::PrevEvent => vec![with(Modifiers::ALT, Key::ArrowLeft)],
            Command::NextEvent => vec![with(Modifiers::ALT, Key::ArrowRight)],
            Command::CursorStart => vec![with(Modifiers::CTRL, Key::Home)],
            Command::CursorEnd => vec![with(Modifiers::CTRL, Key::End)],
        }
    }

    /// Shortcut to show next to the command title.
    pub fn display_shortcut(self) -> Option<KeyboardShortcut> {
        match self {
            Command::Copy => Some(with(Modifiers::COMMAND, Key::C)),
            Command::Paste => Some(with(Modifiers::COMMAND, Key::V)),
            _ => self.shortcuts().into_iter().next(),
        }
    }
}

/// Consume the shortcut pressed in this frame, if it matches any command.
/// Does nothing while a text field is being edited.
pub fn pressed(ctx: &Context) -> Option<Command> {
    if ctx.egui_wants_keyboard_input() {
        return None;
    }
    let mut candidates: Vec<(KeyboardShortcut, Command)> = ALL
        .iter()
        .flat_map(|cmd| cmd.shortcuts().into_iter().map(move |s| (s, *cmd)))
        .collect();
    // egui matches `Ctrl+Shift+Z` also as `Ctrl+Z`, and `Shift+L` as `L`, so test most specific first.
    candidates.sort_by_key(|(s, _)| -(s.modifiers.shift as i32 + s.modifiers.alt as i32));
    ctx.input_mut(|i| {
        candidates
            .iter()
            .find(|(shortcut, _)| i.consume_shortcut(shortcut))
            .map(|(_, cmd)| *cmd)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_lists_each_command_once() {
        let unique: HashSet<_> = ALL.iter().collect();
        assert_eq!(unique.len(), ALL.len());
    }

    #[test]
    fn shortcuts_do_not_clash() {
        let mut seen = HashSet::new();
        for cmd in ALL {
            for s in cmd.shortcuts() {
                assert!(
                    seen.insert((s.logical_key, s.modifiers)),
                    "Shortcut of {:?} is used twice",
                    cmd
                );
            }
        }
    }
}
