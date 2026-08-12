use std::io;

use crate::{
    discovery::{self, Transport},
    protocols::{
        BatteryReading,
        logitech_hidpp::{self, QueryOutcome as LogitechQueryOutcome},
        steelseries_aerox_prime::{self, QueryOutcome as SteelSeriesQueryOutcome},
    },
    transports::{windows_bluetooth::BluetoothSession, windows_hid::HidDevice},
};

#[cfg(debug_assertions)]
use crate::transports::windows_hid::HidReportLengths;

use super::{BatteryProtocol, BatteryState, ConnectionState, DeviceStatus, RecognizedDevice};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatteryPoll {
    Reading(BatteryReading),
    Sleeping,
    ConnectedUnknown,
    Disconnected,
}

enum SessionBackend {
    Hid(HidDevice),
    Bluetooth(BluetoothSession),
}

pub(crate) struct DeviceSession {
    device: RecognizedDevice,
    backend: SessionBackend,
    last_battery: BatteryState,
}

impl DeviceSession {
    pub(crate) fn open(device: RecognizedDevice) -> io::Result<Self> {
        let backend = open_backend(&device)?;

        Ok(Self {
            device,
            backend,
            last_battery: BatteryState::Unknown,
        })
    }

    pub(crate) fn device(&self) -> &RecognizedDevice {
        &self.device
    }

    #[cfg(debug_assertions)]
    pub(crate) fn report_lengths(&self) -> Option<HidReportLengths> {
        match &self.backend {
            SessionBackend::Hid(hid_device) => Some(hid_device.report_lengths()),

            SessionBackend::Bluetooth(_) => None,
        }
    }

    pub(crate) fn rebind(&mut self, device: RecognizedDevice) -> io::Result<()> {
        let backend = open_backend(&device)?;

        self.device = device;
        self.backend = backend;

        Ok(())
    }

    pub(crate) fn poll_status(&mut self) -> DeviceStatus {
        let connection = match self.query_battery() {
            Ok(BatteryPoll::Reading(reading)) => {
                self.last_battery =
                    battery_state_from_reading(self.device.battery_protocol, reading);

                ConnectionState::Connected
            }

            Ok(BatteryPoll::Sleeping) => ConnectionState::Sleeping,

            Ok(BatteryPoll::ConnectedUnknown) => {
                self.last_battery = BatteryState::Unknown;
                ConnectionState::Connected
            }

            Ok(BatteryPoll::Disconnected) => ConnectionState::Disconnected,

            Err(_error) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse status: {} poll failed: {_error}",
                    self.device.name
                );

                ConnectionState::Disconnected
            }
        };

        DeviceStatus {
            name: self.device.name.clone(),
            mode: self.device.connection_mode,
            connection,
            battery: self.last_battery,
        }
    }

    pub(crate) fn query_battery(&mut self) -> io::Result<BatteryPoll> {
        if let SessionBackend::Bluetooth(bluetooth_session) = &self.backend {
            let status = bluetooth_session.poll()?;

            if !status.connected {
                return Ok(BatteryPoll::Disconnected);
            }

            return match status.battery_level {
                Some(level) => Ok(BatteryPoll::Reading(BatteryReading {
                    level,
                    charging: false,
                })),

                None => Ok(BatteryPoll::ConnectedUnknown),
            };
        }

        let outcome = match self.query_hid_once() {
            Ok(outcome) => outcome,

            Err(first_error) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse session: {} HID query failed; rediscovering device: {}",
                    self.device.name, first_error
                );

                self.recover_from_io_failure(&first_error)?;

                self.query_hid_once()?
            }
        };

        Ok(outcome)
    }

    fn query_hid_once(&self) -> io::Result<BatteryPoll> {
        let SessionBackend::Hid(hid_device) = &self.backend else {
            return Err(io::Error::other(
                "Bluetooth session cannot perform a HID query",
            ));
        };

        query_once(hid_device, self.device.battery_protocol)
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

        let replacement = find_replacement_hardware(&self.device, &discovered_hardware)?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    format!(
                        "HID query failed ({first_error}); {} is no longer present",
                        self.device.name
                    ),
                )
            })?;

        let replacement_handle =
            HidDevice::open(&replacement.device_path).map_err(
                |reopen_error| {
                    io::Error::new(
                        reopen_error.kind(),
                        format!(
                            "HID query failed ({first_error}); rediscovered device could not be opened ({reopen_error})"
                        ),
                    )
                },
            )?;

        self.device.hardware = replacement;
        self.backend = SessionBackend::Hid(replacement_handle);

        #[cfg(debug_assertions)]
        eprintln!(
            "BarePulse session: {} rediscovery recovery succeeded: key={}",
            self.device.name, self.device.hardware.hardware_key
        );

        Ok(())
    }
}

fn open_backend(device: &RecognizedDevice) -> io::Result<SessionBackend> {
    match device.hardware.transport {
        Transport::UsbHid => Ok(SessionBackend::Hid(HidDevice::open(
            &device.hardware.device_path,
        )?)),

        Transport::Bluetooth => Ok(SessionBackend::Bluetooth(BluetoothSession::open(
            &device.hardware.hardware_key,
            &device.hardware.device_path,
        )?)),
    }
}

fn battery_state_from_reading(protocol: BatteryProtocol, reading: BatteryReading) -> BatteryState {
    match (protocol, reading.charging) {
        (BatteryProtocol::LogitechHidppAdc, true) => BatteryState::EstimatedCharging(reading.level),

        (BatteryProtocol::LogitechHidppAdc, false) => BatteryState::EstimatedLevel(reading.level),

        (_, true) => BatteryState::Charging(reading.level),

        (_, false) => BatteryState::Level(reading.level),
    }
}

fn query_once(hid_device: &HidDevice, protocol: BatteryProtocol) -> io::Result<BatteryPoll> {
    match protocol {
        BatteryProtocol::SteelSeriesAeroxPrime { command } => {
            let outcome = steelseries_aerox_prime::query(hid_device, command)?;

            classify_steelseries_outcome(command, outcome)
        }

        BatteryProtocol::LogitechHidppAdc => match logitech_hidpp::query(hid_device)? {
            LogitechQueryOutcome::Reading(reading) => Ok(BatteryPoll::Reading(reading)),

            LogitechQueryOutcome::Sleeping => Ok(BatteryPoll::Sleeping),
        },

        BatteryProtocol::WindowsBluetoothBattery => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows Bluetooth battery protocol does not use HID",
        )),
    }
}

fn classify_steelseries_outcome(
    command: u8,
    outcome: SteelSeriesQueryOutcome,
) -> io::Result<BatteryPoll> {
    match outcome {
        SteelSeriesQueryOutcome::Reading(reading) => Ok(BatteryPoll::Reading(reading)),

        SteelSeriesQueryOutcome::NoResponse | SteelSeriesQueryOutcome::UnrelatedReports
            if command == 0xD2 =>
        {
            Ok(BatteryPoll::Sleeping)
        }

        SteelSeriesQueryOutcome::NoResponse => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("wired SteelSeries battery command 0x{command:02X} returned no response"),
        )),

        SteelSeriesQueryOutcome::UnrelatedReports => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "wired SteelSeries battery command 0x{command:02X} received only unrelated HID reports"
            ),
        )),
    }
}

fn find_replacement_hardware(
    current: &RecognizedDevice,
    candidates: &[discovery::DiscoveredHardware],
) -> io::Result<Option<discovery::DiscoveredHardware>> {
    let mut matching = candidates.iter().filter(|candidate| {
        candidate.transport == current.hardware.transport
            && candidate.vendor_id == current.hardware.vendor_id
            && candidate.product_id == current.hardware.product_id
            && candidate.interface_number == current.hardware.interface_number
            && candidate.usage_page == current.hardware.usage_page
            && candidate.usage == current.hardware.usage
            && serials_match(
                current.hardware.serial_number.as_deref(),
                candidate.serial_number.as_deref(),
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
            profile: "steelseries.aerox9".to_string(),
            name: "SteelSeries Aerox 9 Wireless".to_string(),
            connection_mode: if product_id == 0x1858 {
                crate::devices::ConnectionMode::Wireless
            } else {
                crate::devices::ConnectionMode::Wired
            },
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
            classify_steelseries_outcome(0xD2, SteelSeriesQueryOutcome::NoResponse,)
                .expect("wireless no-response should be a sleeping state"),
            BatteryPoll::Sleeping
        );
    }

    #[test]
    fn wireless_unrelated_reports_are_sleeping() {
        assert_eq!(
            classify_steelseries_outcome(0xD2, SteelSeriesQueryOutcome::UnrelatedReports,)
                .expect("wireless unrelated reports should be a sleeping state"),
            BatteryPoll::Sleeping
        );
    }

    #[test]
    fn wired_no_response_is_timeout() {
        let error = classify_steelseries_outcome(0x92, SteelSeriesQueryOutcome::NoResponse)
            .expect_err("wired no-response should not be treated as sleeping");

        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn wired_unrelated_reports_are_protocol_error() {
        let error = classify_steelseries_outcome(0x92, SteelSeriesQueryOutcome::UnrelatedReports)
            .expect_err("wired unrelated reports should not be treated as sleeping");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn logitech_adc_reading_is_estimated() {
        assert_eq!(
            battery_state_from_reading(
                BatteryProtocol::LogitechHidppAdc,
                BatteryReading {
                    level: 75,
                    charging: false,
                },
            ),
            BatteryState::EstimatedLevel(75)
        );

        assert_eq!(
            battery_state_from_reading(
                BatteryProtocol::LogitechHidppAdc,
                BatteryReading {
                    level: 75,
                    charging: true,
                },
            ),
            BatteryState::EstimatedCharging(75)
        );
    }

    #[test]
    fn replacement_may_have_new_device_path() {
        let current = test_device("old-instance", 0x1858, None);

        let replacement = test_device("new-instance", 0x1858, None).hardware;

        let found = find_replacement_hardware(&current, std::slice::from_ref(&replacement))
            .expect("replacement search should succeed")
            .expect("replacement should be found");

        assert_eq!(found, replacement);
    }

    #[test]
    fn replacement_respects_serial_number() {
        let current = test_device("old-instance", 0x1858, Some("mouse-a"));

        let wrong = test_device("other-instance", 0x1858, Some("mouse-b")).hardware;

        assert!(
            find_replacement_hardware(&current, &[wrong])
                .expect("replacement search should succeed")
                .is_none()
        );
    }

    #[test]
    fn ambiguous_replacement_is_rejected() {
        let current = test_device("old-instance", 0x1858, None);

        let first = test_device("first-instance", 0x1858, None).hardware;

        let second = test_device("second-instance", 0x1858, None).hardware;

        let error = find_replacement_hardware(&current, &[first, second])
            .expect_err("ambiguous replacement must be rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
