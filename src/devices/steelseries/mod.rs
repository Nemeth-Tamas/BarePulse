mod aerox9;

use crate::{devices::RecognizedDevice, discovery::DiscoveredHardware};

pub(super) fn recognize(hardware: &[DiscoveredHardware]) -> Vec<RecognizedDevice> {
    hardware.iter().filter_map(aerox9::recognize).collect()
}
