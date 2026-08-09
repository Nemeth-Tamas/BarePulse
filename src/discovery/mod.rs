use std::io;

use crate::transports::windows_hid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    UsbHid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredHardware {
    pub(crate) transport: Transport,
    pub(crate) hardware_key: String,
    pub(crate) vendor_id: Option<u16>,
    pub(crate) product_id: Option<u16>,
    pub(crate) interface_number: Option<u32>,
}

pub(crate) fn discover() -> io::Result<Vec<DiscoveredHardware>> {
    windows_hid::enumerate()
}
