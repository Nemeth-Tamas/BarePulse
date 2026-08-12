use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

const REGISTRY_SCHEMA: u32 = 1;

const AEROX_PROFILE_ID: &str = "steelseries.aerox9";
const AEROX_PROFILE_FILE: &str = "steelseries.aerox9.toml";
const AEROX_PROFILE_SHA256: &str =
    "0abbc78f18c981cc4d2691a9550c660718052d960583e1d00177603adc256486";

const STEELSERIES_VENDOR_ID: u32 = 0x1038;
const AEROX_WIRELESS_PRODUCT_ID: u16 = 0x1858;
const AEROX_WIRED_PRODUCT_ID: u16 = 0x185A;

const CREATIVE_PROFILE_ID: &str = "creative.outlier-free-pro-plus";
const CREATIVE_PROFILE_FILE: &str = "creative.outlier-free-pro-plus.toml";
const CREATIVE_PROFILE_SHA256: &str =
    "a3133c6c58fe9d4cc03ff7e1d657522b2b03f311fca47f2c16a48b2d0345d9d0";

const CREATIVE_VENDOR_ID: u32 = 0x0001_05D6;
const CREATIVE_PRODUCT_ID: u16 = 0x000A;

const MANAGEMENT_INTERFACE: u32 = 3;
const MANAGEMENT_USAGE_PAGE: u16 = 0xFFC0;
const MANAGEMENT_USAGE: u16 = 1;

const WIRELESS_BATTERY_COMMAND: u8 = 0xD2;
const WIRED_BATTERY_COMMAND: u8 = 0x92;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,

    #[serde(default)]
    revision: u64,

    profiles: Vec<ManifestProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestProfile {
    id: String,
    path: String,
    sha256: String,
    matches: Vec<ManifestMatch>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
enum Transport {
    UsbHid,
    Bluetooth,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
struct ManifestMatch {
    transport: Transport,
    vendor_id: u32,
    product_id: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceProfile {
    schema: u32,
    id: String,
    name: String,
    protocol: Protocol,
    connections: Vec<Connection>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Protocol {
    SteelseriesAeroxPrime,
    WindowsBluetoothBattery,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
enum ConnectionMode {
    Wired,
    Wireless,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Connection {
    mode: ConnectionMode,
    transport: Transport,
    vendor_id: u32,
    product_id: u16,

    #[serde(default)]
    interface_number: Option<u32>,

    #[serde(default)]
    usage_page: Option<u16>,

    #[serde(default)]
    usage: Option<u16>,

    #[serde(default)]
    battery_command: Option<u8>,
}

fn devices_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("devices")
}

fn read_toml<T>(path: &Path) -> T
where
    T: for<'de> Deserialize<'de>,
{
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

    toml::from_str(&contents)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

#[test]
fn manifest_is_valid_and_contains_aerox_9() {
    let directory = devices_directory();
    let manifest: Manifest = read_toml(&directory.join("manifest.toml"));

    assert_eq!(manifest.schema, REGISTRY_SCHEMA);
    assert_eq!(manifest.revision, 2);
    assert!(!manifest.profiles.is_empty());

    let mut profile_ids = HashSet::new();
    let mut profile_paths = HashSet::new();

    for profile in &manifest.profiles {
        assert!(!profile.id.trim().is_empty());
        assert!(!profile.path.trim().is_empty());
        assert_eq!(profile.sha256.len(), 64);
        assert!(profile.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(!profile.matches.is_empty());

        assert!(
            profile_ids.insert(profile.id.as_str()),
            "duplicate manifest profile id: {}",
            profile.id
        );

        assert!(
            profile_paths.insert(profile.path.as_str()),
            "duplicate manifest profile path: {}",
            profile.path
        );

        assert!(
            directory.join(&profile.path).is_file(),
            "manifest profile file does not exist: {}",
            profile.path
        );
    }

    let aerox = manifest
        .profiles
        .iter()
        .find(|profile| profile.id == AEROX_PROFILE_ID)
        .expect("Aerox 9 registry entry should exist");

    assert_eq!(aerox.path, AEROX_PROFILE_FILE);
    assert_eq!(aerox.sha256, AEROX_PROFILE_SHA256);

    assert!(aerox.matches.iter().any(|device_match| {
        device_match.transport == Transport::UsbHid
            && device_match.vendor_id == STEELSERIES_VENDOR_ID
            && device_match.product_id == AEROX_WIRELESS_PRODUCT_ID
    }));

    assert!(aerox.matches.iter().any(|device_match| {
        device_match.transport == Transport::UsbHid
            && device_match.vendor_id == STEELSERIES_VENDOR_ID
            && device_match.product_id == AEROX_WIRED_PRODUCT_ID
    }));

    let creative = manifest
        .profiles
        .iter()
        .find(|profile| profile.id == CREATIVE_PROFILE_ID)
        .expect("Creative Outlier Free Pro+ registry entry should exist");

    assert_eq!(creative.path, CREATIVE_PROFILE_FILE);
    assert_eq!(creative.sha256, CREATIVE_PROFILE_SHA256);

    assert!(creative.matches.iter().any(|device_match| {
        device_match.transport == Transport::Bluetooth
            && device_match.vendor_id == CREATIVE_VENDOR_ID
            && device_match.product_id == CREATIVE_PRODUCT_ID
    }));
}

#[test]
fn aerox_9_profile_matches_proven_hardware_contract() {
    let profile: DeviceProfile = read_toml(&devices_directory().join(AEROX_PROFILE_FILE));

    assert_eq!(profile.schema, REGISTRY_SCHEMA);
    assert_eq!(profile.id, AEROX_PROFILE_ID);
    assert_eq!(profile.name, "SteelSeries Aerox 9 Wireless");
    assert_eq!(profile.protocol, Protocol::SteelseriesAeroxPrime);
    assert_eq!(profile.connections.len(), 2);

    let wireless = profile
        .connections
        .iter()
        .find(|connection| connection.mode == ConnectionMode::Wireless)
        .expect("wireless Aerox 9 connection should exist");

    assert_eq!(wireless.transport, Transport::UsbHid);
    assert_eq!(wireless.vendor_id, STEELSERIES_VENDOR_ID);
    assert_eq!(wireless.product_id, AEROX_WIRELESS_PRODUCT_ID);
    assert_eq!(wireless.interface_number, Some(MANAGEMENT_INTERFACE));
    assert_eq!(wireless.usage_page, Some(MANAGEMENT_USAGE_PAGE));
    assert_eq!(wireless.usage, Some(MANAGEMENT_USAGE));
    assert_eq!(wireless.battery_command, Some(WIRELESS_BATTERY_COMMAND));

    let wired = profile
        .connections
        .iter()
        .find(|connection| connection.mode == ConnectionMode::Wired)
        .expect("wired Aerox 9 connection should exist");

    assert_eq!(wired.transport, Transport::UsbHid);
    assert_eq!(wired.vendor_id, STEELSERIES_VENDOR_ID);
    assert_eq!(wired.product_id, AEROX_WIRED_PRODUCT_ID);
    assert_eq!(wired.interface_number, Some(MANAGEMENT_INTERFACE));
    assert_eq!(wired.usage_page, Some(MANAGEMENT_USAGE_PAGE));
    assert_eq!(wired.usage, Some(MANAGEMENT_USAGE));
    assert_eq!(wired.battery_command, Some(WIRED_BATTERY_COMMAND));
}

#[test]
fn creative_outlier_profile_matches_proven_bluetooth_contract() {
    let profile: DeviceProfile = read_toml(&devices_directory().join(CREATIVE_PROFILE_FILE));

    assert_eq!(profile.schema, REGISTRY_SCHEMA);
    assert_eq!(profile.id, CREATIVE_PROFILE_ID);
    assert_eq!(profile.name, "Creative Outlier Free Pro+");
    assert_eq!(profile.protocol, Protocol::WindowsBluetoothBattery);
    assert_eq!(profile.connections.len(), 1);

    let wireless = &profile.connections[0];

    assert_eq!(wireless.mode, ConnectionMode::Wireless);
    assert_eq!(wireless.transport, Transport::Bluetooth);
    assert_eq!(wireless.vendor_id, CREATIVE_VENDOR_ID);
    assert_eq!(wireless.product_id, CREATIVE_PRODUCT_ID);

    assert_eq!(wireless.interface_number, None);
    assert_eq!(wireless.usage_page, None);
    assert_eq!(wireless.usage, None);
    assert_eq!(wireless.battery_command, None);
}

#[test]
fn manifest_matches_are_present_in_the_profile() {
    let directory = devices_directory();

    let manifest: Manifest = read_toml(&directory.join("manifest.toml"));

    let manifest_profile = manifest
        .profiles
        .iter()
        .find(|profile| profile.id == AEROX_PROFILE_ID)
        .expect("Aerox 9 manifest entry should exist");

    let profile: DeviceProfile = read_toml(&directory.join(&manifest_profile.path));

    assert_eq!(manifest_profile.id, profile.id);

    for device_match in &manifest_profile.matches {
        assert!(
            profile.connections.iter().any(|connection| {
                connection.transport == device_match.transport
                    && connection.vendor_id == device_match.vendor_id
                    && connection.product_id == device_match.product_id
            }),
            "manifest match {:04X}:{:04X} is missing from profile {}",
            device_match.vendor_id,
            device_match.product_id,
            profile.id
        );
    }
}

#[test]
fn aerox_profile_has_no_duplicate_connections() {
    let profile: DeviceProfile = read_toml(&devices_directory().join(AEROX_PROFILE_FILE));

    let mut identities = HashSet::new();

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

        assert!(
            identities.insert(identity),
            "duplicate connection in {}",
            profile.id
        );
    }
}
