use std::io;

#[cfg(debug_assertions)]
use std::{env, thread, time::Duration};

use crate::{
    config::{
        ConfigStore,
        model::{Config, DeviceTransport, DiscoveredDevice},
    },
    devices::{self, BatteryPoll, BatteryProtocol, DeviceSession, RecognizedDevice},
    discovery::{self, Transport},
    platform,
};

pub(crate) fn run() -> io::Result<()> {
    let config_store = ConfigStore::discover()?;
    let mut config = config_store.load_or_create()?;

    let discovered_hardware = discovery::discover()?;
    let recognized_devices = devices::recognize(&discovered_hardware);

    if persist_recognized_devices(&mut config, &recognized_devices) {
        config_store.save(&config)?;
    }

    let mut device_sessions = open_device_sessions(&recognized_devices);

    #[cfg(debug_assertions)]
    {
        log_discovery(&discovered_hardware, &recognized_devices);
        probe_device_sessions(&mut device_sessions);

        if reconnect_test_requested() {
            run_reconnect_test(&mut device_sessions);
        }
    }

    let result = platform::windows::run();

    drop(device_sessions);

    result
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

fn open_device_sessions(devices: &[RecognizedDevice]) -> Vec<DeviceSession> {
    devices
        .iter()
        .filter_map(|device| match DeviceSession::open(device.clone()) {
            Ok(session) => Some(session),

            Err(error) => {
                #[cfg(debug_assertions)]
                eprintln!("BarePulse HID open failed: {}: {error}", device.name);

                None
            }
        })
        .collect()
}

#[cfg(debug_assertions)]
const RECONNECT_TEST_POLLS: usize = 30;

#[cfg(debug_assertions)]
const RECONNECT_TEST_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(debug_assertions)]
fn reconnect_test_requested() -> bool {
    env::var_os("BAREPULSE_RECONNECT_TEST").is_some()
}

#[cfg(debug_assertions)]
fn probe_device_sessions(sessions: &mut [DeviceSession]) {
    for session in sessions {
        let report_lengths = session.report_lengths();
        let device_name = session.device().name;

        eprintln!(
            "BarePulse HID open: {} input={} output={} feature={}",
            device_name, report_lengths.input, report_lengths.output, report_lengths.feature,
        );

        let command = match session.device().battery_protocol {
            BatteryProtocol::SteelSeriesAeroxPrime { command } => command,
        };

        match session.query_battery() {
            Ok(BatteryPoll::Reading(reading)) => {
                eprintln!(
                    "BarePulse battery: {} command=0x{command:02X} level={}% charging={}",
                    device_name, reading.level, reading.charging,
                );
            }

            Ok(BatteryPoll::Sleeping) => {
                eprintln!(
                    "BarePulse battery: {} command=0x{command:02X} sleeping",
                    device_name
                );
            }

            Err(error) => {
                eprintln!("BarePulse battery query failed: {}: {error}", device_name);
            }
        }
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
            let device_name = session.device().name;

            let command = match session.device().battery_protocol {
                BatteryProtocol::SteelSeriesAeroxPrime { command } => command,
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
    eprintln!(
        "BarePulse discovery: {} HID interfaces currently present",
        hardware.len()
    );

    for device in hardware
        .iter()
        .filter(|device| device.vendor_id == Some(0x1038))
    {
        eprintln!(
            "  SteelSeries {:?}: VID={:04X} PID={} interface={:?} usage={:?}:{:?} product={:?} serial={:?} key={}",
            device.transport,
            device.vendor_id.unwrap_or_default(),
            device
                .product_id
                .map(|value| format!("{value:04X}"))
                .unwrap_or_else(|| "unknown".to_string()),
            device.interface_number,
            device.usage_page,
            device.usage,
            device.product_string,
            device.serial_number,
            device.hardware_key,
        );
    }

    eprintln!(
        "BarePulse recognition: {} supported device(s)",
        recognized_devices.len()
    );

    for device in recognized_devices {
        eprintln!(
            "  {} [{}]: PID={} interface={:?} usage={:?}:{:?}",
            device.name,
            device.profile,
            device
                .hardware
                .product_id
                .map(|value| format!("{value:04X}"))
                .unwrap_or_else(|| "unknown".to_string()),
            device.hardware.interface_number,
            device.hardware.usage_page,
            device.hardware.usage,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recognized_device() -> RecognizedDevice {
        RecognizedDevice {
            profile: "steelseries.aerox9",
            name: "SteelSeries Aerox 9 Wireless",
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
}
