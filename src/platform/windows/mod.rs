mod tray;
mod window;

pub(crate) use window::run;

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
