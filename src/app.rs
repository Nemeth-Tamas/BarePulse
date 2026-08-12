use std::io;

#[cfg(debug_assertions)]
use std::{env, thread, time::Duration};

use crate::{
    config::{
        ConfigStore,
        model::{Config, DeviceTransport, DiscoveredDevice},
    },
    devices::{self, DeviceSession, DeviceStatus, RecognizedDevice},
    discovery::{self, Transport},
    platform,
};

#[cfg(debug_assertions)]
use crate::devices::BatteryPoll;

#[cfg(any(debug_assertions, test))]
use crate::devices::BatteryProtocol;

pub(crate) fn run() -> io::Result<()> {
    let config_store = ConfigStore::discover()?;
    let mut config = config_store.load_or_create()?;

    let device_registry = devices::DeviceRegistry::discover()?;

    let discovered_hardware = discovery::discover()?;
    let recognized_devices = devices::recognize(&discovered_hardware, &device_registry)?;

    if persist_recognized_devices(&mut config, &recognized_devices) {
        config_store.save(&config)?;
    }

    let mut device_sessions = open_device_sessions(&recognized_devices);

    #[cfg(debug_assertions)]
    {
        log_discovery(&discovered_hardware, &recognized_devices);

        if reconnect_test_requested() {
            run_reconnect_test(&mut device_sessions);
        }
    }

    let initial_statuses = poll_device_statuses(&mut device_sessions);

    #[cfg(debug_assertions)]
    log_device_sessions(&device_sessions, &initial_statuses);

    let poll_interval_seconds = config.settings.poll_interval_seconds;

    platform::windows::run(initial_statuses, poll_interval_seconds, move |reason| {
        if let platform::windows::RefreshReason::HardwareArrival(device_paths) = reason {
            reconcile_after_hardware_arrival(
                &config_store,
                &mut config,
                &mut device_sessions,
                &device_registry,
                &device_paths,
            )?;
        }

        Ok(poll_device_statuses(&mut device_sessions))
    })
}

fn persist_recognized_devices(
    config: &mut Config,
    recognized_devices: &[RecognizedDevice],
) -> bool {
    let mut changed = false;

    for recognized in recognized_devices {
        let mut discovered_device = to_config_device(recognized);

        if let Some(existing) = config.discovered_devices.iter_mut().find(|existing| {
            existing.transport == discovered_device.transport
                && existing.hardware_key == discovered_device.hardware_key
        }) {
            discovered_device.enabled = existing.enabled;

            if *existing != discovered_device {
                *existing = discovered_device;
                changed = true;
            }

            continue;
        }

        config.discovered_devices.push(discovered_device);
        changed = true;
    }

    changed
}

fn to_config_device(recognized: &RecognizedDevice) -> DiscoveredDevice {
    DiscoveredDevice {
        transport: match recognized.hardware.transport {
            Transport::UsbHid => DeviceTransport::UsbHid,
            Transport::Bluetooth => DeviceTransport::Bluetooth,
        },
        hardware_key: recognized.hardware.hardware_key.clone(),
        name: recognized.name.to_string(),
        vendor_id: recognized.hardware.vendor_id,
        product_id: recognized.hardware.product_id,
        interface_number: recognized.hardware.interface_number,
        usage_page: recognized.hardware.usage_page,
        usage: recognized.hardware.usage,
        serial_number: recognized.hardware.serial_number.clone(),
        profile: Some(recognized.profile.to_string()),
        enabled: true,
    }
}

#[cfg(debug_assertions)]
fn open_device_sessions(devices: &[RecognizedDevice]) -> Vec<DeviceSession> {
    devices
        .iter()
        .filter_map(|device| match DeviceSession::open(device.clone()) {
            Ok(session) => Some(session),

            Err(error) => {
                eprintln!("BarePulse device open failed: {}: {error}", device.name);

                None
            }
        })
        .collect()
}

#[cfg(not(debug_assertions))]
fn open_device_sessions(devices: &[RecognizedDevice]) -> Vec<DeviceSession> {
    devices
        .iter()
        .filter_map(|device| DeviceSession::open(device.clone()).ok())
        .collect()
}

fn reconcile_after_hardware_arrival(
    config_store: &ConfigStore,
    config: &mut Config,
    sessions: &mut Vec<DeviceSession>,
    device_registry: &devices::DeviceRegistry,
    device_paths: &[String],
) -> io::Result<()> {
    let discovered_hardware = if device_paths.is_empty() {
        #[cfg(debug_assertions)]
        eprintln!(
            "BarePulse targeted discovery: no arrival paths; \
             falling back to full HID discovery"
        );

        discovery::discover()?
    } else {
        #[cfg(debug_assertions)]
        eprintln!(
            "BarePulse targeted discovery: inspecting {} \
             arriving HID interface(s)",
            device_paths.len()
        );

        match discovery::discover_paths(device_paths) {
            Ok(hardware) => hardware,

            Err(_error) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse targeted discovery failed: {_error}; \
                     falling back to full HID discovery"
                );

                discovery::discover()?
            }
        }
    };

    let recognized_devices = devices::recognize(&discovered_hardware, device_registry)?;

    let config_changed = persist_recognized_devices(config, &recognized_devices);

    let (_rebound_sessions, _added_sessions) =
        reconcile_arrived_device_sessions(sessions, &recognized_devices);

    if config_changed {
        config_store.save(config)?;
    }

    #[cfg(debug_assertions)]
    eprintln!(
        "BarePulse arrival scan: {} supported arrival(s), \
         {_rebound_sessions} rebound session(s), \
         {_added_sessions} new session(s), \
         config_changed={config_changed}",
        recognized_devices.len(),
    );

    Ok(())
}

fn reconcile_arrived_device_sessions(
    sessions: &mut Vec<DeviceSession>,
    recognized_devices: &[RecognizedDevice],
) -> (usize, usize) {
    let mut rebound = 0;
    let mut added = 0;

    for recognized in recognized_devices {
        if let Some(index) = sessions
            .iter()
            .position(|session| same_stable_device_identity(session.device(), recognized))
        {
            match sessions[index].rebind(recognized.clone()) {
                Ok(()) => {
                    rebound += 1;

                    #[cfg(debug_assertions)]
                    eprintln!(
                        "BarePulse arrival scan: rebound {} key={}",
                        recognized.name, recognized.hardware.hardware_key,
                    );
                }

                Err(_error) => {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "BarePulse arrival scan: failed to rebind {}: \
                         {_error}",
                        recognized.name
                    );
                }
            }

            continue;
        }

        match DeviceSession::open(recognized.clone()) {
            Ok(session) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse arrival scan: opened {} PID={}",
                    recognized.name,
                    recognized
                        .hardware
                        .product_id
                        .map(|value| format!("{value:04X}"))
                        .unwrap_or_else(|| "unknown".to_string()),
                );

                sessions.push(session);
                added += 1;
            }

            Err(_error) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse arrival scan: failed to open {}: \
                     {_error}",
                    recognized.name
                );
            }
        }
    }

    (rebound, added)
}

fn same_stable_device_identity(left: &RecognizedDevice, right: &RecognizedDevice) -> bool {
    if left.hardware.transport == right.hardware.transport
        && left.hardware.hardware_key == right.hardware.hardware_key
    {
        return true;
    }

    let (Some(left_serial), Some(right_serial)) = (
        left.hardware.serial_number.as_deref(),
        right.hardware.serial_number.as_deref(),
    ) else {
        return false;
    };

    left_serial == right_serial && same_connection_details(left, right)
}

fn same_connection_details(left: &RecognizedDevice, right: &RecognizedDevice) -> bool {
    left.profile == right.profile
        && left.hardware.transport == right.hardware.transport
        && left.hardware.product_id == right.hardware.product_id
        && left.hardware.interface_number == right.hardware.interface_number
        && left.hardware.usage_page == right.hardware.usage_page
        && left.hardware.usage == right.hardware.usage
}

#[cfg(debug_assertions)]
const RECONNECT_TEST_POLLS: usize = 30;

#[cfg(debug_assertions)]
const RECONNECT_TEST_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(debug_assertions)]
fn reconnect_test_requested() -> bool {
    env::var_os("BAREPULSE_RECONNECT_TEST").is_some()
}

fn poll_device_statuses(sessions: &mut [DeviceSession]) -> Vec<DeviceStatus> {
    sessions
        .iter_mut()
        .map(DeviceSession::poll_status)
        .collect()
}

#[cfg(debug_assertions)]
fn log_device_sessions(sessions: &[DeviceSession], statuses: &[DeviceStatus]) {
    for (session, status) in sessions.iter().zip(statuses) {
        match session.report_lengths() {
            Some(report_lengths) => {
                eprintln!(
                    "BarePulse HID open: {} input={} output={} feature={}",
                    status.name,
                    report_lengths.input,
                    report_lengths.output,
                    report_lengths.feature,
                );
            }

            None => {
                eprintln!("BarePulse Bluetooth open: {}", status.name);
            }
        }

        eprintln!(
            "BarePulse status: {} mode={:?} connection={:?} battery={:?}",
            status.name, status.mode, status.connection, status.battery,
        );
    }
}

#[cfg(debug_assertions)]
fn run_reconnect_test(sessions: &mut [DeviceSession]) {
    if sessions.is_empty() {
        eprintln!("BarePulse reconnect test: no open device sessions");
        return;
    }

    eprintln!(
        "BarePulse reconnect test: starting {RECONNECT_TEST_POLLS} polls; \
         unplug the receiver, wait for at least one failed poll, then reconnect it"
    );

    for poll in 1..=RECONNECT_TEST_POLLS {
        for session in sessions.iter_mut() {
            let device_name = session.device().name.clone();

            let command = match session.device().battery_protocol {
                BatteryProtocol::SteelSeriesAeroxPrime { command } => command,

                BatteryProtocol::LogitechHidppAdc | BatteryProtocol::WindowsBluetoothBattery => {
                    eprintln!(
                        "BarePulse reconnect test: skipping non-SteelSeries device {}",
                        session.device().name
                    );

                    continue;
                }
            };

            match session.query_battery() {
                Ok(BatteryPoll::Reading(reading)) => {
                    eprintln!(
                        "BarePulse reconnect test: poll {poll}/{RECONNECT_TEST_POLLS} \
                         {} command=0x{command:02X} level={}% charging={}",
                        device_name, reading.level, reading.charging,
                    );
                }

                Ok(BatteryPoll::Sleeping) => {
                    eprintln!(
                        "BarePulse reconnect test: poll {poll}/{RECONNECT_TEST_POLLS} \
                         {} command=0x{command:02X} sleeping",
                        device_name
                    );
                }

                Ok(BatteryPoll::ConnectedUnknown) => {
                    eprintln!(
                        "BarePulse reconnect test: poll {poll}/{RECONNECT_TEST_POLLS} \
                         {} connected with unknown battery",
                        device_name
                    );
                }

                Ok(BatteryPoll::Disconnected) => {
                    eprintln!(
                        "BarePulse reconnect test: poll {poll}/{RECONNECT_TEST_POLLS} \
                         {} disconnected",
                        device_name
                    );
                }

                Err(error) => {
                    eprintln!(
                        "BarePulse reconnect test: poll {poll}/{RECONNECT_TEST_POLLS} \
                         {} error={error}",
                        device_name
                    );
                }
            }
        }

        if poll < RECONNECT_TEST_POLLS {
            thread::sleep(RECONNECT_TEST_INTERVAL);
        }
    }

    eprintln!("BarePulse reconnect test: finished");
}

#[cfg(debug_assertions)]
fn log_discovery(
    hardware: &[discovery::DiscoveredHardware],
    recognized_devices: &[RecognizedDevice],
) {
    let hid_count = hardware
        .iter()
        .filter(|device| device.transport == Transport::UsbHid)
        .count();

    let bluetooth_count = hardware
        .iter()
        .filter(|device| device.transport == Transport::Bluetooth)
        .count();

    eprintln!(
        "BarePulse discovery: {hid_count} HID interface(s), \
         {bluetooth_count} Bluetooth device(s)"
    );

    for device in hardware
        .iter()
        .filter(|device| device.transport == Transport::Bluetooth)
    {
        let battery_node = (!device.device_path.is_empty()).then_some(device.device_path.as_str());

        eprintln!(
            "  Bluetooth: name={:?} VID={} PID={} battery_node={:?} key={}",
            device.product_string,
            device
                .vendor_id
                .map(|value| format!("{value:08X}"))
                .unwrap_or_else(|| "unknown".to_string()),
            device
                .product_id
                .map(|value| format!("{value:04X}"))
                .unwrap_or_else(|| "unknown".to_string()),
            battery_node,
            device.hardware_key,
        );
    }

    eprintln!(
        "BarePulse recognition: {} supported device(s)",
        recognized_devices.len()
    );

    for device in recognized_devices {
        eprintln!(
            "  {} [{}]: {:?} VID={} PID={} interface={:?} usage={:?}:{:?} product={:?} serial={:?} key={}",
            device.name,
            device.profile,
            device.hardware.transport,
            device
                .hardware
                .vendor_id
                .map(|value| format!("{value:04X}"))
                .unwrap_or_else(|| "unknown".to_string()),
            device
                .hardware
                .product_id
                .map(|value| format!("{value:04X}"))
                .unwrap_or_else(|| "unknown".to_string()),
            device.hardware.interface_number,
            device.hardware.usage_page,
            device.hardware.usage,
            device.hardware.product_string,
            device.hardware.serial_number,
            device.hardware.hardware_key,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recognized_device() -> RecognizedDevice {
        RecognizedDevice {
            profile: "steelseries.aerox9".to_string(),
            name: "SteelSeries Aerox 9 Wireless".to_string(),
            connection_mode: devices::ConnectionMode::Wired,
            hardware: discovery::DiscoveredHardware {
                transport: Transport::UsbHid,
                hardware_key: "test-aerox".to_string(),
                device_path: r"\\?\hid#test-aerox".to_string(),
                vendor_id: Some(0x1038),
                product_id: Some(0x185A),
                interface_number: Some(3),
                usage_page: Some(0xFFC0),
                usage: Some(1),
                product_string: Some("SteelSeries Aerox 9 Wireless".to_string()),
                serial_number: None,
            },
            battery_protocol: BatteryProtocol::SteelSeriesAeroxPrime { command: 0x92 },
        }
    }

    #[test]
    fn persists_new_recognized_device() {
        let mut config = Config::default();

        assert!(persist_recognized_devices(
            &mut config,
            &[recognized_device()]
        ));

        assert_eq!(config.discovered_devices.len(), 1);

        let persisted = &config.discovered_devices[0];

        assert_eq!(persisted.profile.as_deref(), Some("steelseries.aerox9"));
        assert_eq!(persisted.product_id, Some(0x185A));
        assert_eq!(persisted.interface_number, Some(3));
        assert_eq!(persisted.usage_page, Some(0xFFC0));
        assert_eq!(persisted.usage, Some(1));
    }

    #[test]
    fn unchanged_recognized_device_does_not_trigger_save() {
        let recognized = recognized_device();
        let mut config = Config::default();

        assert!(persist_recognized_devices(
            &mut config,
            std::slice::from_ref(&recognized)
        ));

        assert!(!persist_recognized_devices(
            &mut config,
            std::slice::from_ref(&recognized)
        ));

        assert_eq!(config.discovered_devices.len(), 1);
    }

    #[test]
    fn preserves_enabled_state_when_device_is_rediscovered() {
        let recognized = recognized_device();
        let mut config = Config::default();

        assert!(persist_recognized_devices(
            &mut config,
            std::slice::from_ref(&recognized)
        ));

        config.discovered_devices[0].enabled = false;

        assert!(!persist_recognized_devices(
            &mut config,
            std::slice::from_ref(&recognized)
        ));

        assert!(!config.discovered_devices[0].enabled);
    }

    #[test]
    fn matching_hardware_key_is_stable_identity() {
        let first = recognized_device();
        let second = first.clone();

        assert!(same_stable_device_identity(&first, &second));
    }

    #[test]
    fn changed_key_without_serial_is_not_assumed_same() {
        let first = recognized_device();
        let mut second = first.clone();

        second.hardware.hardware_key = "replacement-aerox-instance".to_string();

        second.hardware.device_path = r"\\?\hid#replacement-aerox-instance".to_string();

        assert!(!same_stable_device_identity(&first, &second));
    }

    #[test]
    fn matching_serial_survives_instance_key_change() {
        let mut first = recognized_device();

        first.hardware.serial_number = Some("TEST-SERIAL".to_string());

        let mut second = first.clone();

        second.hardware.hardware_key = "replacement-aerox-instance".to_string();

        second.hardware.device_path = r"\\?\hid#replacement-aerox-instance".to_string();

        assert!(same_stable_device_identity(&first, &second));
    }

    #[test]
    fn wired_and_wireless_connections_are_distinct() {
        let mut wired = recognized_device();

        wired.hardware.serial_number = Some("TEST-SERIAL".to_string());

        let mut wireless = wired.clone();

        wireless.hardware.hardware_key = "wireless-aerox".to_string();

        wireless.hardware.product_id = Some(0x1858);
        wireless.connection_mode = devices::ConnectionMode::Wireless;

        wireless.battery_protocol = BatteryProtocol::SteelSeriesAeroxPrime { command: 0xD2 };

        assert!(!same_stable_device_identity(&wired, &wireless));
    }
}
