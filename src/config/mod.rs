use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

use serde::{Deserialize, Serialize};

use crate::platform::windows;

const CONFIG_FILE_NAME: &str = "barepulse.toml";
const CONFIG_SCHEMA: u32 = 1;
const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) schema: u32,
    pub(crate) settings: Settings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    pub(crate) poll_interval_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema: CONFIG_SCHEMA,
            settings: Settings::default(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            poll_interval_seconds: DEFAULT_POLL_INTERVAL_SECONDS,
        }
    }
}

impl Config {
    fn validate(&self) -> io::Result<()> {
        if self.schema != CONFIG_SCHEMA {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unsupported BarePulse config schema {}; expected {}",
                    self.schema, CONFIG_SCHEMA
                ),
            ));
        }

        if self.settings.poll_interval_seconds == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "poll_interval_seconds must be greater than zero",
            ));
        }

        Ok(())
    }
}

pub(crate) struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub(crate) fn discover() -> io::Result<Self> {
        let executable = env::current_exe()?;

        let directory = executable
            .parent()
            .ok_or_else(|| io::Error::other("BarePulse executable has no parent directory"))?;

        Ok(Self {
            path: directory.join(CONFIG_FILE_NAME),
        })
    }

    pub(crate) fn load_or_create(&self) -> io::Result<Config> {
        match fs::read_to_string(&self.path) {
            Ok(contents) => parse_config(&contents),

            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let config = Config::default();
                self.save(&config)?;
                Ok(config)
            }

            Err(error) => Err(error),
        }
    }

    pub(crate) fn save(&self, config: &Config) -> io::Result<()> {
        config.validate()?;

        let mut contents = toml::to_string_pretty(config).map_err(io::Error::other)?;

        if !contents.ends_with('\n') {
            contents.push('\n');
        }

        let temporary_path = temporary_path(&self.path);

        let result = (|| {
            write_temporary_file(&temporary_path, contents.as_bytes())?;
            windows::replace_file_atomically(&temporary_path, &self.path)
        })();

        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }

        result
    }
}

fn parse_config(contents: &str) -> io::Result<Config> {
    let config: Config = toml::from_str(contents)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    config.validate()?;

    Ok(config)
}

fn temporary_path(config_path: &Path) -> PathBuf {
    config_path.with_file_name(format!("{CONFIG_FILE_NAME}.{}.tmp", process::id()))
}

fn write_temporary_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;

    file.write_all(contents)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips() {
        let original = Config::default();
        let serialized = toml::to_string_pretty(&original).expect("serialize default config");
        let parsed = parse_config(&serialized).expect("parse serialized config");

        assert_eq!(parsed, original);
    }

    #[test]
    fn rejects_unsupported_schema() {
        let contents = r#"
schema = 99

[settings]
poll_interval_seconds = 300
"#;

        let error = parse_config(contents).expect_err("unsupported schema should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_zero_poll_interval() {
        let contents = r#"
schema = 1

[settings]
poll_interval_seconds = 0
"#;

        let error = parse_config(contents).expect_err("zero poll interval should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
