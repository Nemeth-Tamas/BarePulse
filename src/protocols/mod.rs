pub(crate) mod steelseries_aerox_prime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BatteryReading {
    pub(crate) level: u8,
    pub(crate) charging: bool,
}
