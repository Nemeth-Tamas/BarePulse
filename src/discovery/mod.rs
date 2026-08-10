use std::io;

use crate::transports::windows_hid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    UsbHid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredHardware {
    pub(crate) transport: Transport,
    pub(crate) hardware_key: String,
    pub(crate) device_path: String,
    pub(crate) vendor_id: Option<u16>,
    pub(crate) product_id: Option<u16>,
    pub(crate) interface_number: Option<u32>,
    pub(crate) usage_page: Option<u16>,
    pub(crate) usage: Option<u16>,
    pub(crate) product_string: Option<String>,
    pub(crate) serial_number: Option<String>,
}

pub(crate) fn discover() -> io::Result<Vec<DiscoveredHardware>> {
    windows_hid::enumerate()
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
