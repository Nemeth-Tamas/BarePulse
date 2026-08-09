mod session;
mod status;
mod steelseries;

use crate::discovery::DiscoveredHardware;

pub(crate) use session::{BatteryPoll, DeviceSession};
pub(crate) use status::{BatteryState, ConnectionMode, ConnectionState, DeviceStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatteryProtocol {
    SteelSeriesAeroxPrime { command: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecognizedDevice {
    pub(crate) profile: &'static str,
    pub(crate) name: &'static str,
    pub(crate) connection_mode: ConnectionMode,
    pub(crate) battery_protocol: BatteryProtocol,
    pub(crate) hardware: DiscoveredHardware,
}

pub(crate) fn recognize(hardware: &[DiscoveredHardware]) -> Vec<RecognizedDevice> {
    steelseries::recognize(hardware)
}
