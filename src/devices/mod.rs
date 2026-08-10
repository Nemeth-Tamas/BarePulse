mod registry;
mod session;
mod status;

use std::io;

use crate::discovery::DiscoveredHardware;

pub(crate) use registry::DeviceRegistry;
pub(crate) use session::{BatteryPoll, DeviceSession};
pub(crate) use status::{BatteryState, ConnectionMode, ConnectionState, DeviceStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatteryProtocol {
    SteelSeriesAeroxPrime { command: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecognizedDevice {
    pub(crate) profile: String,
    pub(crate) name: String,
    pub(crate) connection_mode: ConnectionMode,
    pub(crate) battery_protocol: BatteryProtocol,
    pub(crate) hardware: DiscoveredHardware,
}

pub(crate) fn recognize(
    hardware: &[DiscoveredHardware],
    registry: &DeviceRegistry,
) -> io::Result<Vec<RecognizedDevice>> {
    registry.recognize(hardware)
}
