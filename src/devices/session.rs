use std::io;

use crate::{
    devices::{BatteryProtocol, RecognizedDevice},
    protocols::{BatteryReading, steelseries_aerox_prime},
    transports::windows_hid::{HidDevice, HidReportLengths},
};

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

    pub(crate) fn query_battery(&mut self) -> io::Result<Option<BatteryReading>> {
        match query_once(&self.hid_device, self.device.battery_protocol) {
            Ok(reading) => Ok(reading),

            Err(first_error) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse session: {} HID query failed; reopening cached device handle: {}",
                    self.device.name, first_error
                );

                let reopened =
                    HidDevice::open(&self.device.hardware.device_path).map_err(|reopen_error| {
                        io::Error::new(
                            reopen_error.kind(),
                            format!(
                                "HID query failed ({first_error}); reopen failed ({reopen_error})"
                            ),
                        )
                    })?;

                self.hid_device = reopened;

                query_once(&self.hid_device, self.device.battery_protocol)
            }
        }
    }
}

fn query_once(
    hid_device: &HidDevice,
    protocol: BatteryProtocol,
) -> io::Result<Option<BatteryReading>> {
    match protocol {
        BatteryProtocol::SteelSeriesAeroxPrime { command } => {
            steelseries_aerox_prime::query(hid_device, command)
        }
    }
}
