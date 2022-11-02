use std::borrow::{Borrow, BorrowMut};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rodio::Source;

use vst::api::{Event, Events, Supported};
use vst::buffer::AudioBuffer;
use vst::host::{Host, HostBuffer, PluginInstance, PluginLoader};
use vst::host::OpCode::GetSampleRate;
use vst::plugin::{CanDo, Plugin};

// #[allow(dead_code)]
pub struct VstHost;

impl Host for VstHost {
    // fn automate(&self, index: i32, value: f32) {
    //     println!("VST Parameter {} had its value changed to {}", index, value);
    // }
}

pub struct Vst {
    pub host: Arc<Mutex<VstHost>>,
    pub plugin: Arc<Mutex<PluginInstance>>,
    pub sample_rate: f32,
}

impl Vst {
    pub fn init() -> Vst {
        // let path = Path::new("/home/petr/opt/Pianoteq 7/x86-64bit/Pianoteq 7.lv2/Pianoteq_7.so");
        let path = Path::new("/home/petr/opt/Pianoteq 7/x86-64bit/Pianoteq 7.so");
        // let path = Path::new("/usr/lib/VST/amsynth_vst.so");
        println!("Loading {}", path.to_str().unwrap());

        let host = Arc::new(Mutex::new(VstHost));
        let mut loader = PluginLoader::load(path, Arc::clone(&host))
            .unwrap_or_else(|e| panic!("Failed to load plugin: {}", e));
        let mut plugin_holder = Arc::new(Mutex::new(loader.instance().unwrap()));
        let sample_rate = 48000.0;
        {
            let mut plugin = plugin_holder.lock().unwrap();
            plugin.suspend();

            let info = plugin.get_info();
            // Diagnostics: get the plugin information
            println!(
                "Loaded '{}':\n\t\
             Vendor: {}\n\t\
             Presets: {}\n\t\
             Parameters count: {}\n\t\
             VST ID: {}\n\t\
             Version: {}\n\t\
             Initial delay: {} samples\n\t\
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
            plugin.set_sample_rate(sample_rate.to_owned() as f32);
            plugin.set_block_size(128); // Does it affect processing delay?
            plugin.resume();
            plugin.start_process();
        }
        Vst { host, plugin: plugin_holder, sample_rate }
    }
}

pub struct OutputSource {
    sample_idx: usize,
    channel_idx: usize,
    sample_rate: u32,
    outputs: Vec<Vec<f32>>,
    plugin: Arc<Mutex<PluginInstance>>,
    empty: bool,
}

impl OutputSource {
    pub fn new(vst: &Vst, buf_size: usize) -> OutputSource {
        assert!(buf_size > 0);
        let outputs;
        {
            let plugin_holder = vst.plugin.clone();
            let plugin = plugin_holder.try_lock().unwrap();
            let info = plugin.get_info();
            outputs = vec![vec![0.0; buf_size]; info.outputs as usize];
        }
        OutputSource {
            sample_rate: vst.sample_rate.to_owned() as u32,
            sample_idx: 0,
            channel_idx: 0,
            outputs,
            plugin: vst.plugin.clone(),
            empty: true,
        }
    }

    fn fill_buffer(&mut self) {
        let mut plugin = self.plugin.lock().unwrap();
        let info = plugin.get_info();
        let output_count = info.outputs as usize;
        let input_count = info.inputs as usize;
        let inputs = vec![vec![0.0; 0]; input_count];
        let mut host_buffer: HostBuffer<f32> = HostBuffer::new(input_count, output_count);
        let mut buffer = host_buffer.bind(&inputs, &mut self.outputs);

        plugin.process(&mut buffer);
        self.sample_idx = 0;
        self.channel_idx = 0;
    }
}

impl Iterator for OutputSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.empty {
            self.fill_buffer();
            self.empty = false;
        }
        let mut_outputs = &mut self.outputs;
        let mut output = mut_outputs.get_mut(self.channel_idx.to_owned());
        if output == None {
            /* Channels are interleaved (see https://github.com/RustAudio/rodio/blob/master/src/source/channel_volume.rs)
               So for 2 channels we have to put 2 samples in sequence */
            self.channel_idx = 0;
            self.sample_idx += 1;
            output = mut_outputs.get_mut(self.channel_idx.to_owned());
        }
        let mut sample = output.unwrap().get(self.sample_idx.to_owned());
        match sample {
            Some(&x) => {
                self.channel_idx += 1;
                Some(x)
            }
            None => {
                self.empty = true;
                self.next()
            }
        }
    }
}

impl Source for OutputSource {
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.outputs.len() as u16
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.sample_rate.to_owned()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}