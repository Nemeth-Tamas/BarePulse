#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionMode {
    Wired,
    Wireless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionState {
    Disconnected,
    Sleeping,
    Connected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatteryState {
    Unknown,
    Level(u8),
    Charging(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceStatus {
    pub(crate) name: String,
    pub(crate) mode: ConnectionMode,
    pub(crate) connection: ConnectionState,
    pub(crate) battery: BatteryState,
}
