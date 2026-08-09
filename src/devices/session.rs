use std::io;

use crate::{
    discovery,
    protocols::{
        BatteryReading,
        steelseries_aerox_prime::{self, QueryOutcome},
    },
    transports::windows_hid::{HidDevice, HidReportLengths},
};

use super::{BatteryProtocol, RecognizedDevice, recognize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatteryPoll {
    Reading(BatteryReading),
    Sleeping,
}

pub(crate) struct DeviceSession {
    device: RecognizedDevice,
    hid_device: HidDevice,
}

impl DeviceSession {
    pub(crate) fn open(device: RecognizedDevice) -> io::Result<Self> {
        let hid_device = HidDevice::open(&device.hardware.device_path)?;

        Ok(Self { device, hid_device })
    }

    pub(crate) fn device(&self) -> &RecognizedDevice {
        &self.device
    }

    pub(crate) const fn report_lengths(&self) -> HidReportLengths {
        self.hid_device.report_lengths()
    }

    pub(crate) fn query_battery(&mut self) -> io::Result<BatteryPoll> {
        let outcome = match query_once(&self.hid_device, self.device.battery_protocol) {
            Ok(outcome) => outcome,

            Err(first_error) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse session: {} HID query failed; rediscovering device: {}",
                    self.device.name, first_error
                );

                self.recover_from_io_failure(&first_error)?;

                query_once(&self.hid_device, self.device.battery_protocol)?
            }
        };

        classify_outcome(self.device.battery_protocol, outcome)
    }

    fn recover_from_io_failure(&mut self, first_error: &io::Error) -> io::Result<()> {
        let discovered_hardware = discovery::discover().map_err(|rediscovery_error| {
            io::Error::new(
                rediscovery_error.kind(),
                format!(
                    "HID query failed ({first_error}); hardware rediscovery failed ({rediscovery_error})"
                ),
            )
        })?;

        let recognized_devices = recognize(&discovered_hardware);

        let replacement =
            find_replacement(&self.device, &recognized_devices)?.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    format!(
                        "HID query failed ({first_error}); {} is no longer present",
                        self.device.name
                    ),
                )
            })?;

        let replacement_handle =
            HidDevice::open(&replacement.hardware.device_path).map_err(|reopen_error| {
                io::Error::new(
                    reopen_error.kind(),
                    format!(
                        "HID query failed ({first_error}); rediscovered device could not be opened ({reopen_error})"
                    ),
                )
            })?;

        self.device = replacement;
        self.hid_device = replacement_handle;

        Ok(())
    }
}

fn query_once(hid_device: &HidDevice, protocol: BatteryProtocol) -> io::Result<QueryOutcome> {
    match protocol {
        BatteryProtocol::SteelSeriesAeroxPrime { command } => {
            steelseries_aerox_prime::query(hid_device, command)
        }
    }
}

fn classify_outcome(protocol: BatteryProtocol, outcome: QueryOutcome) -> io::Result<BatteryPoll> {
    match outcome {
        QueryOutcome::Reading(reading) => Ok(BatteryPoll::Reading(reading)),

        QueryOutcome::NoResponse => match protocol {
            BatteryProtocol::SteelSeriesAeroxPrime { command: 0xD2 } => Ok(BatteryPoll::Sleeping),

            BatteryProtocol::SteelSeriesAeroxPrime { command } => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("wired SteelSeries battery command 0x{command:02X} returned no response"),
            )),
        },
    }
}

fn find_replacement(
    current: &RecognizedDevice,
    candidates: &[RecognizedDevice],
) -> io::Result<Option<RecognizedDevice>> {
    let mut matching = candidates.iter().filter(|candidate| {
        candidate.profile == current.profile
            && candidate.hardware.product_id == current.hardware.product_id
            && serials_match(
                current.hardware.serial_number.as_deref(),
                candidate.hardware.serial_number.as_deref(),
            )
    });

    let Some(first) = matching.next() else {
        return Ok(None);
    };

    if matching.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "multiple rediscovered devices match {} and cannot be distinguished safely",
                current.name
            ),
        ));
    }

    Ok(Some(first.clone()))
}

fn serials_match(current: Option<&str>, candidate: Option<&str>) -> bool {
    match (current, candidate) {
        (Some(current), Some(candidate)) => current == candidate,
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{DiscoveredHardware, Transport};

    fn test_device(
        hardware_key: &str,
        product_id: u16,
        serial_number: Option<&str>,
    ) -> RecognizedDevice {
        RecognizedDevice {
            profile: "steelseries.aerox9",
            name: "SteelSeries Aerox 9 Wireless",
            battery_protocol: BatteryProtocol::SteelSeriesAeroxPrime {
                command: if product_id == 0x1858 { 0xD2 } else { 0x92 },
            },
            hardware: DiscoveredHardware {
                transport: Transport::UsbHid,
                hardware_key: hardware_key.to_string(),
                device_path: format!(r"\\?\hid#{hardware_key}"),
                vendor_id: Some(0x1038),
                product_id: Some(product_id),
                interface_number: Some(3),
                usage_page: Some(0xFFC0),
                usage: Some(1),
                product_string: Some("SteelSeries Aerox 9 Wireless".to_string()),
                serial_number: serial_number.map(str::to_string),
            },
        }
    }

    #[test]
    fn wireless_no_response_is_sleeping() {
        assert_eq!(
            classify_outcome(
                BatteryProtocol::SteelSeriesAeroxPrime { command: 0xD2 },
                QueryOutcome::NoResponse,
            )
            .expect("wireless no-response should be a sleeping state"),
            BatteryPoll::Sleeping
        );
    }

    #[test]
    fn wired_no_response_is_timeout() {
        let error = classify_outcome(
            BatteryProtocol::SteelSeriesAeroxPrime { command: 0x92 },
            QueryOutcome::NoResponse,
        )
        .expect_err("wired no-response should not be treated as sleeping");

        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn replacement_may_have_new_device_path() {
        let current = test_device("old-instance", 0x1858, None);
        let replacement = test_device("new-instance", 0x1858, None);

        let found = find_replacement(&current, &[replacement.clone()])
            .expect("replacement search should succeed")
            .expect("replacement should be found");

        assert_eq!(found, replacement);
    }

    #[test]
    fn replacement_respects_serial_number() {
        let current = test_device("old-instance", 0x1858, Some("mouse-a"));
        let wrong = test_device("other-instance", 0x1858, Some("mouse-b"));

        assert!(
            find_replacement(&current, &[wrong])
                .expect("replacement search should succeed")
                .is_none()
        );
    }

    #[test]
    fn ambiguous_replacement_is_rejected() {
        let current = test_device("old-instance", 0x1858, None);
        let first = test_device("first-instance", 0x1858, None);
        let second = test_device("second-instance", 0x1858, None);

        let error = find_replacement(&current, &[first, second])
            .expect_err("ambiguous replacement must be rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
