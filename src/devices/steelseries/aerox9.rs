use crate::{
    devices::{BatteryProtocol, RecognizedDevice},
    discovery::{DiscoveredHardware, Transport},
};

const PROFILE_ID: &str = "steelseries.aerox9";
const DEVICE_NAME: &str = "SteelSeries Aerox 9 Wireless";

const STEELSERIES_VENDOR_ID: u16 = 0x1038;

const WIRELESS_PRODUCT_ID: u16 = 0x1858;
const WIRED_PRODUCT_ID: u16 = 0x185A;

const WIRELESS_BATTERY_COMMAND: u8 = 0xD2;
const WIRED_BATTERY_COMMAND: u8 = 0x92;

const MANAGEMENT_INTERFACE: u32 = 3;
const MANAGEMENT_USAGE_PAGE: u16 = 0xFFC0;
const MANAGEMENT_USAGE: u16 = 1;

pub(super) fn recognize(hardware: &DiscoveredHardware) -> Option<RecognizedDevice> {
    if hardware.transport != Transport::UsbHid {
        return None;
    }

    if hardware.vendor_id != Some(STEELSERIES_VENDOR_ID) {
        return None;
    }

    let battery_command = match hardware.product_id {
        Some(WIRELESS_PRODUCT_ID) => WIRELESS_BATTERY_COMMAND,
        Some(WIRED_PRODUCT_ID) => WIRED_BATTERY_COMMAND,
        _ => return None,
    };

    if hardware.interface_number != Some(MANAGEMENT_INTERFACE) {
        return None;
    }

    if hardware.usage_page != Some(MANAGEMENT_USAGE_PAGE)
        || hardware.usage != Some(MANAGEMENT_USAGE)
    {
        return None;
    }

    Some(RecognizedDevice {
        profile: PROFILE_ID,
        name: DEVICE_NAME,
        battery_protocol: BatteryProtocol::SteelSeriesAeroxPrime {
            command: battery_command,
        },
        hardware: hardware.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_hardware(product_id: u16) -> DiscoveredHardware {
        DiscoveredHardware {
            transport: Transport::UsbHid,
            hardware_key: format!(r"HID\VID_1038&PID_{product_id:04X}&MI_03\TEST"),
            device_path: format!(r"\\?\hid#vid_1038&pid_{product_id:04x}&mi_03#test"),
            vendor_id: Some(STEELSERIES_VENDOR_ID),
            product_id: Some(product_id),
            interface_number: Some(MANAGEMENT_INTERFACE),
            usage_page: Some(MANAGEMENT_USAGE_PAGE),
            usage: Some(MANAGEMENT_USAGE),
            product_string: Some(DEVICE_NAME.to_string()),
            serial_number: None,
        }
    }

    #[test]
    fn recognizes_wired_aerox_9_management_interface() {
        let device = recognize(&test_hardware(WIRED_PRODUCT_ID))
            .expect("wired Aerox 9 should be recognized");

        assert_eq!(device.profile, PROFILE_ID);
        assert_eq!(device.name, DEVICE_NAME);
        assert_eq!(
            device.battery_protocol,
            BatteryProtocol::SteelSeriesAeroxPrime {
                command: WIRED_BATTERY_COMMAND,
            }
        );
    }

    #[test]
    fn recognizes_wireless_aerox_9_management_interface() {
        let device = recognize(&test_hardware(WIRELESS_PRODUCT_ID))
            .expect("wireless Aerox 9 should be recognized");

        assert_eq!(device.profile, PROFILE_ID);
        assert_eq!(device.name, DEVICE_NAME);
        assert_eq!(
            device.battery_protocol,
            BatteryProtocol::SteelSeriesAeroxPrime {
                command: WIRELESS_BATTERY_COMMAND,
            }
        );
    }

    #[test]
    fn rejects_non_management_interface() {
        let mut hardware = test_hardware(WIRED_PRODUCT_ID);
        hardware.interface_number = Some(4);
        hardware.usage_page = Some(0xFFC1);

        assert!(recognize(&hardware).is_none());
    }

    #[test]
    fn rejects_wrong_usage() {
        let mut hardware = test_hardware(WIRED_PRODUCT_ID);
        hardware.usage_page = Some(1);
        hardware.usage = Some(2);

        assert!(recognize(&hardware).is_none());
    }

    #[test]
    fn rejects_unknown_product() {
        let hardware = test_hardware(0xFFFF);

        assert!(recognize(&hardware).is_none());
    }
}
