mod steelseries;

use crate::discovery::DiscoveredHardware;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecognizedDevice {
    pub(crate) profile: &'static str,
    pub(crate) name: &'static str,
    pub(crate) hardware: DiscoveredHardware,
}

pub(crate) fn recognize(hardware: &[DiscoveredHardware]) -> Vec<RecognizedDevice> {
    steelseries::recognize(hardware)
}
