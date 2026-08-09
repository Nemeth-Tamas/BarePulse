use std::io;

use crate::{config::ConfigStore, discovery, platform};

pub(crate) fn run() -> io::Result<()> {
    let config_store = ConfigStore::discover()?;
    let _config = config_store.load_or_create()?;

    let discovered_hardware = discovery::discover()?;

    #[cfg(debug_assertions)]
    log_discovery(&discovered_hardware);

    platform::windows::run()
}

#[cfg(debug_assertions)]
fn log_discovery(devices: &[discovery::DiscoveredHardware]) {
    eprintln!(
        "BarePulse discovery: {} HID interfaces currently present",
        devices.len()
    );

    for device in devices
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
}
