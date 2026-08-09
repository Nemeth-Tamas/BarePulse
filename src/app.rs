use std::io;

use crate::{
    config::{
        ConfigStore,
        model::{Config, DeviceTransport, DiscoveredDevice},
    },
    devices::{self, RecognizedDevice},
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

    #[cfg(debug_assertions)]
    log_discovery(&discovered_hardware, &recognized_devices);

    platform::windows::run()
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
