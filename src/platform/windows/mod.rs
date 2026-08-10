mod device_events;
mod file;
mod tray;
mod window;

pub(crate) use file::replace_atomically as replace_file_atomically;
pub(crate) use window::{RefreshReason, run};

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
