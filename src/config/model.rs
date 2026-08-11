use std::{collections::HashSet, io};

use serde::{Deserialize, Serialize};

pub(super) const CONFIG_SCHEMA: u32 = 1;

const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) schema: u32,
    pub(crate) settings: Settings,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) discovered_devices: Vec<DiscoveredDevice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    pub(crate) poll_interval_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DeviceTransport {
    UsbHid,
    Bluetooth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiscoveredDevice {
    pub(crate) transport: DeviceTransport,

    // Opaque stable identity supplied by the transport/discovery layer.
    // BarePulse must not infer semantics from this value.
    pub(crate) hardware_key: String,

    pub(crate) name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) vendor_id: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) product_id: Option<u16>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) interface_number: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) usage_page: Option<u16>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) usage: Option<u16>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) serial_number: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) profile: Option<String>,

    #[serde(default = "default_enabled")]
    pub(crate) enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema: CONFIG_SCHEMA,
            settings: Settings::default(),
            discovered_devices: Vec::new(),
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
    pub(super) fn validate(&self) -> io::Result<()> {
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

        let mut identities = HashSet::new();

        for device in &self.discovered_devices {
            device.validate()?;

            if !identities.insert((device.transport, device.hardware_key.as_str())) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "duplicate discovered device identity: {}",
                        device.hardware_key
                    ),
                ));
            }
        }

        Ok(())
    }
}

impl DiscoveredDevice {
    fn validate(&self) -> io::Result<()> {
        if self.hardware_key.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "discovered device hardware_key must not be empty",
            ));
        }

        if self.name.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "discovered device name must not be empty",
            ));
        }

        if self
            .profile
            .as_deref()
            .is_some_and(|profile| profile.trim().is_empty())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "discovered device profile must not be empty when present",
            ));
        }

        Ok(())
    }
}

const fn default_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_hid_device() -> DiscoveredDevice {
        DiscoveredDevice {
            transport: DeviceTransport::UsbHid,
            hardware_key: "test-hid-device".to_string(),
            name: "Test HID Device".to_string(),
            vendor_id: Some(0x1038),
            product_id: Some(0x1858),
            interface_number: Some(3),
            usage_page: Some(1),
            usage: Some(2),
            serial_number: None,
            profile: Some("steelseries.aerox9".to_string()),
            enabled: true,
        }
    }

    #[test]
    fn default_config_is_valid() {
        Config::default()
            .validate()
            .expect("default config should be valid");
    }

    #[test]
    fn discovered_device_round_trips() {
        let mut original = Config::default();
        original.discovered_devices.push(test_hid_device());

        let serialized =
            toml::to_string_pretty(&original).expect("serialize config with discovered device");

        let parsed: Config =
            toml::from_str(&serialized).expect("parse config with discovered device");

        parsed.validate().expect("parsed config should be valid");

        assert_eq!(parsed, original);
    }

    #[test]
    fn old_config_without_device_list_remains_valid() {
        let contents = r#"
schema = 1

[settings]
poll_interval_seconds = 300
"#;

        let parsed: Config = toml::from_str(contents).expect("parse old config");

        parsed.validate().expect("old config should remain valid");

        assert!(parsed.discovered_devices.is_empty());
    }

    #[test]
    fn duplicate_hardware_identity_is_rejected() {
        let device = test_hid_device();

        let config = Config {
            discovered_devices: vec![device.clone(), device],
            ..Config::default()
        };

        let error = config
            .validate()
            .expect_err("duplicate hardware identities should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn empty_hardware_key_is_rejected() {
        let mut device = test_hid_device();
        device.hardware_key.clear();

        let config = Config {
            discovered_devices: vec![device],
            ..Config::default()
        };

        let error = config
            .validate()
            .expect_err("empty hardware key should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
