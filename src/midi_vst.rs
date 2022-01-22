use std::path::Path;
use std::sync::{Arc, Mutex};

use vst::api::{Events, Supported};
use vst::host::{Host, HostBuffer, PluginLoader};
use vst::plugin::{CanDo, Plugin};

#[allow(dead_code)]
pub struct VstHost;

impl Host for VstHost {
    fn automate(&self, index: i32, value: f32) {
        println!("Parameter {} had its value changed to {}", index, value);
    }
}

impl VstHost {
    pub fn init() -> PluginLoader<VstHost> {
        // let path = Path::new("/home/petr/opt/Pianoteq 7/x86-64bit/Pianoteq 7.lv2/Pianoteq_7.so");
        let path = Path::new("/home/petr/opt/Pianoteq 7/x86-64bit/Pianoteq 7.so");
        println!("Loading {}...", path.to_str().unwrap());

        let host = Arc::new(Mutex::new(VstHost));
        let mut plugin =
            PluginLoader::load(path, Arc::clone(&host))
                .unwrap_or_else(|e| panic!("Failed to load plugin: {}", e));

        // Create an instance of the plugin
        let mut instance = plugin.instance().unwrap();

        // Get the plugin information
        let info = instance.get_info();
        println!(
            "Loaded '{}':\n\t\
             Vendor: {}\n\t\
             Presets: {}\n\t\
             Parameters: {}\n\t\
             VST ID: {}\n\t\
             Version: {}\n\t\
             Initial Delay: {} samples\n\t\
             Inputs {}\n\t\
             Outputs {}",
            info.name, info.vendor, info.presets, info.parameters, info.unique_id,
            info.version, info.initial_delay, info.inputs, info.outputs
        );

        // Initialize the instance
        instance.init();
        println!("Initialized VST instance.");
        println!("Can receive MIDI events {}", instance.can_do(CanDo::ReceiveMidiEvent) == Supported::Yes);

        plugin
    }
}
