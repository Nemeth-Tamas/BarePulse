use std::{
    io,
    time::{Duration, Instant},
};

use crate::{protocols::BatteryReading, transports::windows_hid::HidDevice};

const REPORT_ID_LONG: u8 = 0x11;
const DEVICE_INDEX_RECEIVER: u8 = 0xFF;

/*
 * HID++ reserves the low nibble of the function/client byte
 * for the software ID. G HUB traffic in our capture used 0x0B,
 * so BarePulse uses 0x0E to make its own replies easy to match.
 */
const SOFTWARE_ID: u8 = 0x0E;

const HIDPP20_ERROR: u8 = 0xFF;

const FEATURE_ROOT: u8 = 0x00;
const ROOT_GET_FEATURE: u8 = 0x00;

const FEATURE_BATTERY_LEVEL_STATUS: u16 = 0x1000;
const FEATURE_UNIFIED_BATTERY: u16 = 0x1004;

const UNIFIED_GET_CAPABILITIES: u8 = 0x00;
const UNIFIED_GET_STATUS: u8 = 0x10;

const BATTERY_LEVEL_GET_STATUS: u8 = 0x00;

const WRITE_TIMEOUT: Duration = Duration::from_millis(250);

const READ_SLICE: Duration = Duration::from_millis(50);

const RESPONSE_TIMEOUT: Duration = Duration::from_millis(750);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatteryFeature {
    Unified,
    BatteryLevelStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BatteryProbe {
    pub(crate) feature: BatteryFeature,
    pub(crate) feature_index: u8,
    pub(crate) reading: BatteryReading,
    pub(crate) raw_status: u8,
}

pub(crate) fn probe_battery(device: &HidDevice) -> io::Result<BatteryProbe> {
    require_long_reports(device)?;

    if let Some(feature_index) = get_feature_index(device, FEATURE_UNIFIED_BATTERY)? {
        return probe_unified_battery(device, feature_index);
    }

    if let Some(feature_index) = get_feature_index(device, FEATURE_BATTERY_LEVEL_STATUS)? {
        return probe_battery_level_status(device, feature_index);
    }

    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Logitech device exposes neither HID++ battery feature 0x1004 nor 0x1000",
    ))
}

fn require_long_reports(device: &HidDevice) -> io::Result<()> {
    let lengths = device.report_lengths();

    if lengths.input != 20 || lengths.output != 20 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Logitech HID++ collection has input/output lengths {}/{}; expected 20/20",
                lengths.input, lengths.output
            ),
        ));
    }

    Ok(())
}

fn get_feature_index(device: &HidDevice, feature: u16) -> io::Result<Option<u8>> {
    let params = [(feature >> 8) as u8, feature as u8];

    let response = send_fap_command(device, FEATURE_ROOT, ROOT_GET_FEATURE, &params)?;

    let feature_index = response[4];

    #[cfg(debug_assertions)]
    eprintln!("BarePulse Logitech HID++: feature 0x{feature:04X} index=0x{feature_index:02X}");

    if feature_index == 0 {
        Ok(None)
    } else {
        Ok(Some(feature_index))
    }
}

fn probe_unified_battery(device: &HidDevice, feature_index: u8) -> io::Result<BatteryProbe> {
    let capabilities = send_fap_command(device, feature_index, UNIFIED_GET_CAPABILITIES, &[])?;

    /*
     * HID++ 0x1004 capabilities:
     *   param 0 = supported discrete battery levels
     *   param 1 = flags
     *
     * Bit 1 indicates state-of-charge percentage support.
     */
    let supports_percentage = capabilities[5] & 0x02 != 0;

    #[cfg(debug_assertions)]
    eprintln!(
        "BarePulse Logitech HID++: unified battery capabilities levels=0x{:02X} flags=0x{:02X} percentage={supports_percentage}",
        capabilities[4], capabilities[5],
    );

    let response = send_fap_command(device, feature_index, UNIFIED_GET_STATUS, &[])?;

    let reading = decode_unified_status(&response[4..])?;

    Ok(BatteryProbe {
        feature: BatteryFeature::Unified,
        feature_index,
        reading,
        raw_status: response[6],
    })
}

fn probe_battery_level_status(device: &HidDevice, feature_index: u8) -> io::Result<BatteryProbe> {
    let response = send_fap_command(device, feature_index, BATTERY_LEVEL_GET_STATUS, &[])?;

    let reading = decode_battery_level_status(&response[4..])?;

    Ok(BatteryProbe {
        feature: BatteryFeature::BatteryLevelStatus,
        feature_index,
        reading,
        raw_status: response[6],
    })
}

fn decode_unified_status(params: &[u8]) -> io::Result<BatteryReading> {
    if params.len() < 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HID++ unified battery response is too short",
        ));
    }

    let level = params[0];
    let charging_status = params[2];

    if level > 100 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("HID++ unified battery level {level} exceeds 100%"),
        ));
    }

    let charging = matches!(charging_status, 1 | 2);

    let level = if charging_status == 3 { 100 } else { level };

    Ok(BatteryReading { level, charging })
}

fn decode_battery_level_status(params: &[u8]) -> io::Result<BatteryReading> {
    if params.len() < 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HID++ battery-level response is too short",
        ));
    }

    let mut level = params[0];
    let charging_status = params[2];

    if level > 100 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("HID++ battery level {level} exceeds 100%"),
        ));
    }

    let charging = matches!(charging_status, 1 | 2 | 4);

    if charging_status == 3 {
        level = 100;
    }

    Ok(BatteryReading { level, charging })
}

fn send_fap_command(
    device: &HidDevice,
    feature_index: u8,
    function_index: u8,
    params: &[u8],
) -> io::Result<Vec<u8>> {
    if params.len() > 16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HID++ long report parameter payload exceeds 16 bytes",
        ));
    }

    let mut request = vec![0u8; 20];

    request[0] = REPORT_ID_LONG;
    request[1] = DEVICE_INDEX_RECEIVER;
    request[2] = feature_index;
    request[3] = function_index | SOFTWARE_ID;

    request[4..4 + params.len()].copy_from_slice(params);

    #[cfg(debug_assertions)]
    eprintln!("BarePulse Logitech HID++ tx: {request:?}");

    device.write_report(&request, WRITE_TIMEOUT)?;

    let deadline = Instant::now() + RESPONSE_TIMEOUT;

    while Instant::now() < deadline {
        let Some(response) = device.read_report(READ_SLICE)? else {
            continue;
        };

        if response.len() < 6
            || response[0] != REPORT_ID_LONG
            || response[1] != DEVICE_INDEX_RECEIVER
        {
            continue;
        }

        if response[2] == feature_index && response[3] == request[3] {
            #[cfg(debug_assertions)]
            eprintln!("BarePulse Logitech HID++ rx: {response:?}");

            return Ok(response);
        }

        /*
         * HID++ 2.0 error reply:
         *
         * [0x11, device, 0xFF,
         *  requested feature index,
         *  requested function/client,
         *  error code, ...]
         */
        if response[2] == HIDPP20_ERROR && response[3] == feature_index && response[4] == request[3]
        {
            let error = response[5];

            #[cfg(debug_assertions)]
            eprintln!("BarePulse Logitech HID++ error rx: {response:?}");

            return Err(io::Error::other(format!(
                "Logitech HID++ error 0x{error:02X}"
            )));
        }
    }

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "timed out waiting for Logitech HID++ feature 0x{feature_index:02X} function 0x{function_index:02X}"
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_status_decodes_discharging() {
        assert_eq!(
            decode_unified_status(&[75, 0x04, 0, 0,]).expect("valid unified battery"),
            BatteryReading {
                level: 75,
                charging: false,
            }
        );
    }

    #[test]
    fn unified_status_decodes_charging() {
        assert_eq!(
            decode_unified_status(&[75, 0x04, 1, 1,]).expect("valid unified battery"),
            BatteryReading {
                level: 75,
                charging: true,
            }
        );
    }

    #[test]
    fn battery_level_status_decodes_discharging() {
        assert_eq!(
            decode_battery_level_status(&[75, 70, 0,]).expect("valid battery level"),
            BatteryReading {
                level: 75,
                charging: false,
            }
        );
    }

    #[test]
    fn battery_level_status_decodes_charging() {
        assert_eq!(
            decode_battery_level_status(&[75, 80, 1,]).expect("valid battery level"),
            BatteryReading {
                level: 75,
                charging: true,
            }
        );
    }
}
