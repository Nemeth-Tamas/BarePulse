use std::{
    collections::HashSet,
    env, fs, io,
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;

use crate::{
    discovery::{DiscoveredHardware, Transport as DiscoveryTransport},
    transports::windows_web,
};

use super::{BatteryProtocol, ConnectionMode, RecognizedDevice};

const REGISTRY_SCHEMA: u32 = 1;
const MANIFEST_FILE_NAME: &str = "manifest.toml";

const REGISTRY_GITHUB_HOST: &str = "raw.githubusercontent.com";
const REGISTRY_GITHUB_MANIFEST_PATH: &str = "/Nemeth-Tamas/BarePulse/main/devices/manifest.toml";

const MAXIMUM_MANIFEST_BYTES: usize = 128 * 1024;

pub(crate) struct DeviceRegistry {
    manifest: Manifest,
    directory: PathBuf,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceProfile {
    schema: u32,
    id: String,
    name: String,
    protocol: RegistryProtocol,
    connections: Vec<ProfileConnection>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum RegistryProtocol {
    SteelseriesAeroxPrime,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
enum RegistryConnectionMode {
    Wired,
    Wireless,
}

impl RegistryConnectionMode {
    const fn to_runtime(self) -> ConnectionMode {
        match self {
            Self::Wired => ConnectionMode::Wired,
            Self::Wireless => ConnectionMode::Wireless,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
struct ProfileConnection {
    mode: RegistryConnectionMode,
    transport: RegistryTransport,
    vendor_id: u16,
    product_id: u16,
    interface_number: u32,
    usage_page: u16,
    usage: u16,
    battery_command: u8,
}

impl ManifestProfile {
    fn supports(&self, hardware: &DiscoveredHardware) -> bool {
        let (Some(vendor_id), Some(product_id)) = (hardware.vendor_id, hardware.product_id) else {
            return false;
        };

        self.matches.iter().any(|device_match| {
            device_match.transport.matches(hardware.transport)
                && device_match.vendor_id == vendor_id
                && device_match.product_id == product_id
        })
    }
}

impl ProfileConnection {
    fn matches(&self, hardware: &DiscoveredHardware) -> bool {
        self.transport.matches(hardware.transport)
            && hardware.vendor_id == Some(self.vendor_id)
            && hardware.product_id == Some(self.product_id)
            && hardware.interface_number == Some(self.interface_number)
            && hardware.usage_page == Some(self.usage_page)
            && hardware.usage == Some(self.usage)
    }
}

impl DeviceProfile {
    fn recognize(&self, hardware: &DiscoveredHardware) -> Option<RecognizedDevice> {
        let connection = self
            .connections
            .iter()
            .find(|connection| connection.matches(hardware))?;

        let battery_protocol = match self.protocol {
            RegistryProtocol::SteelseriesAeroxPrime => BatteryProtocol::SteelSeriesAeroxPrime {
                command: connection.battery_command,
            },
        };

        Some(RecognizedDevice {
            profile: self.id.clone(),
            name: self.name.clone(),
            connection_mode: connection.mode.to_runtime(),
            battery_protocol,
            hardware: hardware.clone(),
        })
    }
}

impl DeviceRegistry {
    pub(crate) fn discover() -> io::Result<Self> {
        let executable = env::current_exe()?;

        let executable_directory = executable
            .parent()
            .ok_or_else(|| io::Error::other("BarePulse executable has no parent directory"))?;

        let portable_directory = executable_directory.join("devices");

        let mut registry = match Self::load_from_directory(&portable_directory) {
            Ok(registry) => registry,

            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Self::load_debug_fallback(error)?
            }

            Err(error) => return Err(error),
        };

        registry.refresh_manifest_from_github();

        Ok(registry)
    }

    fn refresh_manifest_from_github(&mut self) {
        #[cfg(debug_assertions)]
        if env::var_os("BAREPULSE_REGISTRY_OFFLINE_TEST").is_some() {
            eprintln!("BarePulse registry: remote manifest fetch skipped by offline test");

            return;
        }

        let contents = match windows_web::get_https_text(
            REGISTRY_GITHUB_HOST,
            REGISTRY_GITHUB_MANIFEST_PATH,
            MAXIMUM_MANIFEST_BYTES,
        ) {
            Ok(contents) => contents,

            Err(error) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse registry: GitHub manifest unavailable: \
                 {error}; using local manifest"
                );

                return;
            }
        };

        let manifest = match parse_manifest(&contents) {
            Ok(manifest) => manifest,

            Err(error) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse registry: GitHub manifest is invalid: \
                 {error}; using local manifest"
                );

                return;
            }
        };

        self.manifest = manifest;

        #[cfg(debug_assertions)]
        eprintln!("BarePulse registry: fetched and validated manifest from GitHub");
    }

    #[cfg(test)]
    pub(crate) fn supports(&self, hardware: &DiscoveredHardware) -> bool {
        self.manifest
            .profiles
            .iter()
            .any(|profile| profile.supports(hardware))
    }

    pub(crate) fn recognize(
        &self,
        hardware: &[DiscoveredHardware],
    ) -> io::Result<Vec<RecognizedDevice>> {
        let mut recognized = Vec::new();

        for manifest_profile in &self.manifest.profiles {
            let candidates = hardware
                .iter()
                .filter(|hardware| manifest_profile.supports(hardware))
                .collect::<Vec<_>>();

            if candidates.is_empty() {
                continue;
            }

            let profile = self.load_profile(manifest_profile)?;

            recognized.extend(
                candidates
                    .into_iter()
                    .filter_map(|hardware| profile.recognize(hardware)),
            );
        }

        Ok(recognized)
    }

    fn load_profile(&self, manifest_profile: &ManifestProfile) -> io::Result<DeviceProfile> {
        let profile_path = self.directory.join(&manifest_profile.path);

        let contents = fs::read_to_string(&profile_path)?;

        let profile = parse_profile(&contents, manifest_profile).map_err(|error| {
            io::Error::new(error.kind(), format!("{}: {error}", profile_path.display()))
        })?;

        #[cfg(debug_assertions)]
        eprintln!(
            "BarePulse registry: loaded profile {} from {}",
            profile.id,
            profile_path.display()
        );

        Ok(profile)
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

        Ok(Self {
            manifest,
            directory: directory.to_path_buf(),
        })
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

fn parse_profile(contents: &str, manifest_profile: &ManifestProfile) -> io::Result<DeviceProfile> {
    let profile: DeviceProfile = toml::from_str(contents)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    validate_profile(manifest_profile, &profile)?;

    Ok(profile)
}

fn validate_profile(manifest_profile: &ManifestProfile, profile: &DeviceProfile) -> io::Result<()> {
    if profile.schema != REGISTRY_SCHEMA {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported device profile schema {}; expected {}",
                profile.schema, REGISTRY_SCHEMA
            ),
        ));
    }

    if profile.id != manifest_profile.id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "device profile id {} does not match manifest id {}",
                profile.id, manifest_profile.id
            ),
        ));
    }

    if profile.name.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("device profile {} has an empty name", profile.id),
        ));
    }

    if profile.connections.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("device profile {} contains no connections", profile.id),
        ));
    }

    let mut connection_identities = HashSet::new();
    let mut coarse_matches = HashSet::new();

    for connection in &profile.connections {
        let identity = (
            connection.mode,
            connection.transport,
            connection.vendor_id,
            connection.product_id,
            connection.interface_number,
            connection.usage_page,
            connection.usage,
        );

        if !connection_identities.insert(identity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "device profile {} contains a duplicate connection",
                    profile.id
                ),
            ));
        }

        let coarse_match = ManifestMatch {
            transport: connection.transport,
            vendor_id: connection.vendor_id,
            product_id: connection.product_id,
        };

        if !manifest_profile.matches.contains(&coarse_match) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "device profile {} connection {:04X}:{:04X} \
                     is missing from the registry manifest",
                    profile.id, connection.vendor_id, connection.product_id
                ),
            ));
        }

        coarse_matches.insert(coarse_match);
    }

    for device_match in &manifest_profile.matches {
        if !coarse_matches.contains(device_match) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "registry manifest match {:04X}:{:04X} \
                     is missing from device profile {}",
                    device_match.vendor_id, device_match.product_id, profile.id
                ),
            ));
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
            directory: PathBuf::new(),
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

    #[test]
    fn profile_constructs_wireless_runtime_device() {
        let manifest = parse_manifest(VALID_MANIFEST).expect("valid test manifest");

        const VALID_PROFILE: &str = r#"
schema = 1

id = "steelseries.aerox9"
name = "SteelSeries Aerox 9 Wireless"
protocol = "steelseries-aerox-prime"

[[connections]]
mode = "wireless"
transport = "usb-hid"
vendor_id = 0x1038
product_id = 0x1858
interface_number = 3
usage_page = 0xFFC0
usage = 1
battery_command = 0xD2

[[connections]]
mode = "wired"
transport = "usb-hid"
vendor_id = 0x1038
product_id = 0x185A
interface_number = 3
usage_page = 0xFFC0
usage = 1
battery_command = 0x92
"#;

        let profile =
            parse_profile(VALID_PROFILE, &manifest.profiles[0]).expect("valid test device profile");

        let recognized = profile
            .recognize(&hardware(0x1038, 0x1858))
            .expect("wireless Aerox should match profile");

        assert_eq!(recognized.profile, "steelseries.aerox9");
        assert_eq!(recognized.name, "SteelSeries Aerox 9 Wireless");
        assert_eq!(recognized.connection_mode, ConnectionMode::Wireless);
        assert_eq!(
            recognized.battery_protocol,
            BatteryProtocol::SteelSeriesAeroxPrime { command: 0xD2 }
        );
    }

    #[test]
    fn profile_rejects_non_management_interface() {
        let manifest = parse_manifest(VALID_MANIFEST).expect("valid test manifest");

        const VALID_PROFILE: &str = r#"
schema = 1

id = "steelseries.aerox9"
name = "SteelSeries Aerox 9 Wireless"
protocol = "steelseries-aerox-prime"

[[connections]]
mode = "wireless"
transport = "usb-hid"
vendor_id = 0x1038
product_id = 0x1858
interface_number = 3
usage_page = 0xFFC0
usage = 1
battery_command = 0xD2

[[connections]]
mode = "wired"
transport = "usb-hid"
vendor_id = 0x1038
product_id = 0x185A
interface_number = 3
usage_page = 0xFFC0
usage = 1
battery_command = 0x92
"#;

        let profile =
            parse_profile(VALID_PROFILE, &manifest.profiles[0]).expect("valid test device profile");

        let mut wrong_interface = hardware(0x1038, 0x1858);

        wrong_interface.interface_number = Some(4);

        assert!(profile.recognize(&wrong_interface).is_none());
    }
}
