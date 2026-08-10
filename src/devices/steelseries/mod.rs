mod aerox9;

use crate::{devices::RecognizedDevice, discovery::DiscoveredHardware};

pub(super) fn recognize(hardware: &DiscoveredHardware) -> Option<RecognizedDevice> {
    aerox9::recognize(hardware)
}
