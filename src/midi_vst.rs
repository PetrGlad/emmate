use std::borrow::{Borrow, BorrowMut};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rodio::Source;

use vst::api::{Event, Events, Supported};
use vst::host::{Host, HostBuffer, PluginInstance, PluginLoader};
use vst::plugin::{CanDo, Plugin};

#[allow(dead_code)]
pub struct VstHost;

impl Host for VstHost {
    // fn automate(&self, index: i32, value: f32) {
    //     println!("VST Parameter {} had its value changed to {}", index, value);
    // }
}

pub struct Vst {
    pub host: Arc<Mutex<VstHost>>,
    pub plugin: PluginInstance,
}

impl Vst {
    pub fn init() -> Vst {
        // let path = Path::new("/home/petr/opt/Pianoteq 7/x86-64bit/Pianoteq 7.lv2/Pianoteq_7.so");
        // let path = Path::new("/home/petr/opt/Pianoteq 7/x86-64bit/Pianoteq 7.so");
        let path = Path::new("/usr/lib/vst/amsynth_vst.so");
        println!("Loading {}", path.to_str().unwrap());

        let host = Arc::new(Mutex::new(VstHost));
        let mut loader = PluginLoader::load(path, Arc::clone(&host))
            .unwrap_or_else(|e| panic!("Failed to load plugin: {}", e));
        let plugin = loader.instance().unwrap();
        let mut vst = Vst { host, plugin };
        // Diagnostics: get the plugin information
        let plugin = &mut vst.plugin;
        plugin.suspend();

        let info = plugin.get_info();
        println!(
            "Loaded '{}':\n\t\
             Vendor: {}\n\t\
             Presets: {}\n\t\
             Parameters: {}\n\t\
             VST ID: {}\n\t\
             Version: {}\n\t\
             Initial delay: {} samples\n\t
             Inputs {}\n\t\
             Outputs {}",
            info.name, info.vendor, info.presets, info.parameters, info.unique_id,
            info.version, info.initial_delay, info.inputs, info.outputs
        );
        let params = plugin.get_parameter_object();
        params.change_preset(4);
        println!("Current preset #{}: {}", params.get_preset_num(), params.get_preset_name(params.get_preset_num()));
        // Initialize the instance

        plugin.init();
        println!("Initialized VST instance.");
        println!("Can receive MIDI events {}", plugin.can_do(CanDo::ReceiveMidiEvent) == Supported::Yes);

        // plugin.suspend();
        plugin.set_sample_rate(48000f32); // rodio expects this
        // plugin.set_block_size(256); // Need it? What does it affect?
        plugin.resume();
        plugin.start_process();

        vst
    }

    // pub fn process_events(mut self, events: &dyn IntoIterator) {
    //     events_buffer.send_events_to_plugin(events, &mut self);
    //
    //     let mut audio_buffer = host_buffer.bind(&inputs, &mut outputs);
    //     let mut instance = self.instance().unwrap();
    //     instance.process(&mut audio_buffer);
    // }
}

impl Iterator for VstHost {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        // self.num_sample = self.num_sample.wrapping_add(1);
        //
        // let value = 2.0 * PI * self.freq * self.num_sample as f32 / 48000.0;

        // TODO Implement
        // Channels are interleaved (see https://github.com/RustAudio/rodio/blob/master/src/source/channel_volume.rs)
        // So for 2 channels we have to put 2 samples in sequence
        // Some(value.sin()); // ch 1
        // Some(value.sin()) // ch 2
        Some(0.0)
    }
}

impl Source for VstHost {
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    #[inline]
    fn channels(&self) -> u16 {
        2
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        48000
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}