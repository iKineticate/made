use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub static CONFIG: LazyLock<Mutex<Config>> =
    LazyLock::new(|| Mutex::new(Config::open().expect("Failed to open config")));

pub static CONFIG_PATH: LazyLock<PathBuf> = LazyLock::new(|| EXE_PATH.with_file_name("made.toml"));

pub static EXE_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| std::env::current_exe().expect("Failed to get made.exe path"));

pub static EXE_NAME: LazyLock<String> = LazyLock::new(|| {
    Path::new(&*EXE_PATH)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_owned())
        .expect("Failed to get EXE name")
});

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub texts: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            texts: Vec::with_capacity(200),
        }
    }
}

impl Config {
    pub fn open() -> Result<Self> {
        let default_config = Config::default();

        Config::read().or_else(|_e| {
            let toml_str = toml::to_string_pretty(&default_config)?;
            std::fs::write(&*CONFIG_PATH, toml_str)?;
            Ok(default_config)
        })
    }

    fn read() -> Result<Self> {
        let content = std::fs::read_to_string(&*CONFIG_PATH)?;
        let toml_config: Config = toml::from_str(&content)?;
        Ok(toml_config)
    }

    pub fn save(&self) {
        let toml_str = toml::to_string_pretty(self)
            .expect("Failed to serialize ConfigToml structure as a String of TOML.");
        std::fs::write(&*CONFIG_PATH, toml_str)
            .expect("Failed to write TOML String to CapsGlow.toml");
    }

    pub fn push_text(&mut self, text: String) {
        if !self.texts.contains(&text) {
            self.texts.push(text.trim().to_owned());
            self.save();
        }
    }
}
