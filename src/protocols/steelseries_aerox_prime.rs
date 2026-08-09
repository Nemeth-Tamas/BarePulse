use std::{io, thread, time::Duration};

use crate::{protocols::BatteryReading, transports::windows_hid::HidDevice};

const CHARGING_FLAG: u8 = 0x80;

const WRITE_ATTEMPTS: usize = 3;
const READ_ATTEMPTS_PER_WRITE: usize = 6;
const MAX_STALE_REPORTS: usize = 8;

const WRITE_TIMEOUT: Duration = Duration::from_millis(250);
const READ_TIMEOUT: Duration = Duration::from_millis(100);
const RETRY_DELAY: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryOutcome {
    Reading(BatteryReading),
    NoResponse,
    UnrelatedReports,
}

pub(crate) fn query(device: &HidDevice, command: u8) -> io::Result<QueryOutcome> {
    drain_stale_reports(device)?;

    let output_length = usize::from(device.report_lengths().output);

    if output_length < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HID output report is too short for SteelSeries battery command",
        ));
    }

    let mut request = vec![0u8; output_length];
    request[0] = 0x00;
    request[1] = command;

    let mut received_any_report = false;

    for write_attempt in 0..WRITE_ATTEMPTS {
        match device.write_report(&request, WRITE_TIMEOUT) {
            Ok(()) => {}

            Err(error) if write_attempt + 1 < WRITE_ATTEMPTS => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse battery: command 0x{command:02X} write attempt {} failed: {error}",
                    write_attempt + 1
                );

                thread::sleep(RETRY_DELAY);
                continue;
            }

            Err(error) => return Err(error),
        }

        for _ in 0..READ_ATTEMPTS_PER_WRITE {
            let Some(report) = device.read_report(READ_TIMEOUT)? else {
                continue;
            };

            received_any_report = true;

            #[cfg(debug_assertions)]
            eprintln!(
                "BarePulse battery raw: command=0x{command:02X} bytes={:?}",
                &report[..report.len().min(8)]
            );

            if let Some(reading) = decode_response(command, &report) {
                return Ok(QueryOutcome::Reading(reading));
            }
        }

        if write_attempt + 1 < WRITE_ATTEMPTS {
            thread::sleep(RETRY_DELAY);
        }
    }

    if received_any_report {
        Ok(QueryOutcome::UnrelatedReports)
    } else {
        Ok(QueryOutcome::NoResponse)
    }
}

fn drain_stale_reports(device: &HidDevice) -> io::Result<()> {
    for _ in 0..MAX_STALE_REPORTS {
        if device.read_report(Duration::ZERO)?.is_none() {
            break;
        }
    }

    Ok(())
}

pub(crate) fn decode_response(command: u8, report: &[u8]) -> Option<BatteryReading> {
    let battery_byte = if report.first().copied() == Some(command) {
        if report.len() >= 3 && report[1] == 0x00 {
            report[2]
        } else {
            *report.get(1)?
        }
    } else if report.first().copied() == Some(0x00) && report.get(1).copied() == Some(command) {
        if report.len() >= 4 && report[2] == 0x00 {
            report[3]
        } else {
            *report.get(2)?
        }
    } else {
        return None;
    };

    if battery_byte == 0 {
        return None;
    }

    let charging = battery_byte & CHARGING_FLAG != 0;
    let raw_level = battery_byte & !CHARGING_FLAG;

    if raw_level == 0 {
        return None;
    }

    let level = if raw_level > 21 {
        raw_level.min(100)
    } else {
        ((u16::from(raw_level.saturating_sub(1)) * 5).min(100)) as u8
    };

    Some(BatteryReading { level, charging })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_standard_windows_response() {
        assert_eq!(
            decode_response(0x92, &[0x92, 0x00, 21]),
            Some(BatteryReading {
                level: 100,
                charging: false,
            })
        );
    }

    #[test]
    fn decodes_windows_response_with_report_id() {
        assert_eq!(
            decode_response(0x92, &[0x00, 0x92, 0x00, 11]),
            Some(BatteryReading {
                level: 50,
                charging: false,
            })
        );
    }

    #[test]
    fn decodes_charging_flag() {
        assert_eq!(
            decode_response(0x92, &[0x00, 0x92, 0x00, 0x80 | 11]),
            Some(BatteryReading {
                level: 50,
                charging: true,
            })
        );
    }

    #[test]
    fn decodes_short_response() {
        assert_eq!(
            decode_response(0xD2, &[0xD2, 10]),
            Some(BatteryReading {
                level: 45,
                charging: false,
            })
        );
    }

    #[test]
    fn accepts_direct_percentage_value() {
        assert_eq!(
            decode_response(0xD2, &[0x00, 0xD2, 75]),
            Some(BatteryReading {
                level: 75,
                charging: false,
            })
        );
    }

    #[test]
    fn rejects_wrong_command_echo() {
        assert_eq!(decode_response(0x92, &[0xD2, 0x00, 10]), None);
    }

    #[test]
    fn rejects_missing_battery_byte() {
        assert_eq!(decode_response(0x92, &[0x92]), None);
        assert_eq!(decode_response(0x92, &[0x00, 0x92]), None);
    }

    #[test]
    fn rejects_zero_battery_byte() {
        assert_eq!(decode_response(0x92, &[0x92, 0x00, 0x00]), None);
    }

    #[test]
    fn ignores_unrelated_report() {
        let report = [0x00, 0x01, 0x02, 0x03, 0x04];

        assert_eq!(decode_response(0x92, &report), None);
    }

    #[test]
    fn decodes_captured_wired_charging_packet() {
        // Captured from a real Aerox 9 Wireless in wired charging mode.
        let report = [0x00, 0x92, 0x95, 0x00, 0x00, 0x00, 0x00, 0x00];

        assert_eq!(
            decode_response(0x92, &report),
            Some(BatteryReading {
                level: 100,
                charging: true,
            })
        );
    }

    #[test]
    fn decodes_captured_wireless_packet() {
        // Captured from a real Aerox 9 Wireless over its 2.4 GHz receiver.
        let report = [0x00, 0xD2, 0x15, 0x00, 0x00, 0x00, 0x00, 0x00];

        assert_eq!(
            decode_response(0xD2, &report),
            Some(BatteryReading {
                level: 100,
                charging: false,
            })
        );
    }

    #[test]
    fn ignores_captured_sleeping_receiver_packet() {
        // Captured from the Aerox 9 receiver while the wireless mouse was asleep.
        let report = [0x00, 0x40, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00];

        assert_eq!(decode_response(0xD2, &report), None);
    }
}
