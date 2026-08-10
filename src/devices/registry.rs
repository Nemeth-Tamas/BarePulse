use std::{
    collections::HashSet,
    env, fs, io,
    path::{Component, Path},
};

use serde::Deserialize;

use crate::discovery::{DiscoveredHardware, Transport as DiscoveryTransport};

const REGISTRY_SCHEMA: u32 = 1;
const MANIFEST_FILE_NAME: &str = "manifest.toml";

pub(crate) struct DeviceRegistry {
    manifest: Manifest,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    profiles: Vec<ManifestProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestProfile {
    id: String,
    path: String,
    matches: Vec<ManifestMatch>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
enum RegistryTransport {
    UsbHid,
}

impl RegistryTransport {
    const fn matches(self, transport: DiscoveryTransport) -> bool {
        matches!(
            (self, transport),
            (Self::UsbHid, DiscoveryTransport::UsbHid)
        )
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
struct ManifestMatch {
    transport: RegistryTransport,
    vendor_id: u16,
    product_id: u16,
}

impl DeviceRegistry {
    pub(crate) fn discover() -> io::Result<Self> {
        let executable = env::current_exe()?;

        let executable_directory = executable
            .parent()
            .ok_or_else(|| io::Error::other("BarePulse executable has no parent directory"))?;

        let portable_directory = executable_directory.join("devices");

        match Self::load_from_directory(&portable_directory) {
            Ok(registry) => Ok(registry),

            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Self::load_debug_fallback(error)
            }

            Err(error) => Err(error),
        }
    }

    pub(crate) fn supports(&self, hardware: &DiscoveredHardware) -> bool {
        let (Some(vendor_id), Some(product_id)) = (hardware.vendor_id, hardware.product_id) else {
            return false;
        };

        self.manifest.profiles.iter().any(|profile| {
            profile.matches.iter().any(|device_match| {
                device_match.transport.matches(hardware.transport)
                    && device_match.vendor_id == vendor_id
                    && device_match.product_id == product_id
            })
        })
    }

    fn load_from_directory(directory: &Path) -> io::Result<Self> {
        let manifest_path = directory.join(MANIFEST_FILE_NAME);

        let contents = fs::read_to_string(&manifest_path)?;

        let manifest = parse_manifest(&contents).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("{}: {error}", manifest_path.display()),
            )
        })?;

        #[cfg(debug_assertions)]
        eprintln!("BarePulse registry: loaded {}", manifest_path.display());

        Ok(Self { manifest })
    }

    #[cfg(debug_assertions)]
    fn load_debug_fallback(_portable_error: io::Error) -> io::Result<Self> {
        let source_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("devices");

        Self::load_from_directory(&source_directory)
    }

    #[cfg(not(debug_assertions))]
    fn load_debug_fallback(portable_error: io::Error) -> io::Result<Self> {
        Err(portable_error)
    }
}

fn parse_manifest(contents: &str) -> io::Result<Manifest> {
    let manifest: Manifest = toml::from_str(contents)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    validate_manifest(&manifest)?;

    Ok(manifest)
}

fn validate_manifest(manifest: &Manifest) -> io::Result<()> {
    if manifest.schema != REGISTRY_SCHEMA {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported device registry schema {}; expected {}",
                manifest.schema, REGISTRY_SCHEMA
            ),
        ));
    }

    if manifest.profiles.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "device registry contains no profiles",
        ));
    }

    let mut profile_ids = HashSet::new();
    let mut profile_paths = HashSet::new();
    let mut hardware_matches = HashSet::new();

    for profile in &manifest.profiles {
        if profile.id.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "device registry contains an empty profile id",
            ));
        }

        if !profile_ids.insert(profile.id.as_str()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("duplicate device registry profile id: {}", profile.id),
            ));
        }

        if profile.path.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("device registry profile {} has an empty path", profile.id),
            ));
        }

        let profile_path = Path::new(&profile.path);

        if profile_path.is_absolute()
            || profile_path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "device registry profile {} has unsafe path {}",
                    profile.id, profile.path
                ),
            ));
        }

        if !profile_paths.insert(profile.path.as_str()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("duplicate device registry profile path: {}", profile.path),
            ));
        }

        if profile.matches.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "device registry profile {} contains no hardware matches",
                    profile.id
                ),
            ));
        }

        for device_match in &profile.matches {
            let identity = (
                device_match.transport,
                device_match.vendor_id,
                device_match.product_id,
            );

            if !hardware_matches.insert(identity) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "duplicate device registry match {:04X}:{:04X}",
                        device_match.vendor_id, device_match.product_id
                    ),
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::Transport;

    const VALID_MANIFEST: &str = r#"
schema = 1

[[profiles]]
id = "steelseries.aerox9"
path = "steelseries.aerox9.toml"

[[profiles.matches]]
transport = "usb-hid"
vendor_id = 0x1038
product_id = 0x1858

[[profiles.matches]]
transport = "usb-hid"
vendor_id = 0x1038
product_id = 0x185A
"#;

    fn hardware(vendor_id: u16, product_id: u16) -> DiscoveredHardware {
        DiscoveredHardware {
            transport: Transport::UsbHid,
            hardware_key: "test-device".to_string(),
            device_path: r"\\?\hid#test-device".to_string(),
            vendor_id: Some(vendor_id),
            product_id: Some(product_id),
            interface_number: Some(3),
            usage_page: Some(0xFFC0),
            usage: Some(1),
            product_string: None,
            serial_number: None,
        }
    }

    fn registry() -> DeviceRegistry {
        DeviceRegistry {
            manifest: parse_manifest(VALID_MANIFEST).expect("valid test manifest"),
        }
    }

    #[test]
    fn registry_accepts_known_wireless_hardware() {
        assert!(registry().supports(&hardware(0x1038, 0x1858)));
    }

    #[test]
    fn registry_accepts_known_wired_hardware() {
        assert!(registry().supports(&hardware(0x1038, 0x185A)));
    }

    #[test]
    fn registry_rejects_unknown_hardware() {
        assert!(!registry().supports(&hardware(0x1038, 0xFFFF)));
    }

    #[test]
    fn registry_rejects_unsupported_schema() {
        let invalid = VALID_MANIFEST.replace("schema = 1", "schema = 99");

        let error = parse_manifest(&invalid).expect_err("unsupported schema should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn registry_rejects_unsafe_profile_path() {
        let invalid =
            VALID_MANIFEST.replace("steelseries.aerox9.toml", "../steelseries.aerox9.toml");

        let error = parse_manifest(&invalid).expect_err("unsafe profile path should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn registry_rejects_duplicate_hardware_match() {
        let invalid = format!(
            "{VALID_MANIFEST}\n\
             [[profiles]]\n\
             id = \"duplicate\"\n\
             path = \"duplicate.toml\"\n\
             [[profiles.matches]]\n\
             transport = \"usb-hid\"\n\
             vendor_id = 0x1038\n\
             product_id = 0x1858\n"
        );

        let error = parse_manifest(&invalid).expect_err("duplicate match should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
