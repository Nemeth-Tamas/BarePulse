use std::io;

use crate::transports::{windows_bluetooth, windows_hid};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    UsbHid,
    Bluetooth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredHardware {
    pub(crate) transport: Transport,
    pub(crate) hardware_key: String,
    pub(crate) device_path: String,
    pub(crate) vendor_id: Option<u32>,
    pub(crate) product_id: Option<u16>,
    pub(crate) interface_number: Option<u32>,
    pub(crate) usage_page: Option<u16>,
    pub(crate) usage: Option<u16>,
    pub(crate) product_string: Option<String>,
    pub(crate) serial_number: Option<String>,
}

pub(crate) fn discover() -> io::Result<Vec<DiscoveredHardware>> {
    let mut hardware = windows_hid::enumerate()?;

    match windows_bluetooth::enumerate() {
        Ok(devices) => {
            hardware.extend(devices.into_iter().map(bluetooth_hardware));
        }

        Err(_error) => {
            #[cfg(debug_assertions)]
            eprintln!("BarePulse discovery: Bluetooth enumeration failed: {_error}");
        }
    }

    Ok(hardware)
}

fn bluetooth_hardware(device: windows_bluetooth::BluetoothDevice) -> DiscoveredHardware {
    let windows_bluetooth::BluetoothDevice {
        name,
        address,
        battery_instance_id,
        vendor_id,
        product_id,
    } = device;

    DiscoveredHardware {
        transport: Transport::Bluetooth,
        hardware_key: format!("{address:012X}"),
        device_path: battery_instance_id.unwrap_or_default(),
        vendor_id,
        product_id,
        interface_number: None,
        usage_page: None,
        usage: None,
        product_string: Some(name),
        serial_number: None,
    }
}

pub(crate) fn discover_paths(paths: &[String]) -> io::Result<Vec<DiscoveredHardware>> {
    let mut hardware = Vec::new();
    let mut inspected_any = false;
    let mut first_error = None;

    for path in paths {
        match windows_hid::inspect_path(path) {
            Ok(Some(device)) => {
                inspected_any = true;
                hardware.push(device);
            }

            Ok(None) => {
                inspected_any = true;
            }

            Err(error) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse targeted discovery: \
                     failed to inspect {path}: {error}"
                );

                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }

    if !paths.is_empty() && !inspected_any {
        return Err(first_error
            .unwrap_or_else(|| io::Error::other("all targeted HID interface inspections failed")));
    }

    Ok(hardware)
}
