use std::path::PathBuf;

use serde::Deserialize;

pub const DEFAUL_CONFIG_TOML: &str = include_str!("default-config.toml");

#[derive(Deserialize)]
pub struct Config {
    pub vst_plugin_path: String,
}

impl Config {
    pub(crate) fn load(config_path: Option<&PathBuf>) -> Config {
        let toml_str = match config_path {
            None => DEFAUL_CONFIG_TOML.into(),
            Some(path) => std::fs::read_to_string(path)
                .expect(format!("Cannot load config file {:?}", path).as_str()),
        };
        toml::from_str(&toml_str)
            .expect(format!("Cannot parse config toml {:?}", toml_str).as_str())
    }
}
