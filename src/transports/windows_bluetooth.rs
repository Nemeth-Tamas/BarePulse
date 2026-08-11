use std::{
    io,
    mem::{size_of, zeroed},
};

use windows_sys::Win32::{
    Devices::Bluetooth::{
        BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS, BluetoothFindDeviceClose,
        BluetoothFindFirstDevice, BluetoothFindNextDevice, HBLUETOOTH_DEVICE_FIND,
    },
    Foundation::{ERROR_NO_MORE_ITEMS, GetLastError},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BluetoothDevice {
    pub(crate) name: String,
    pub(crate) connected: bool,
    pub(crate) remembered: bool,
    pub(crate) authenticated: bool,
}

struct BluetoothFindHandle(HBLUETOOTH_DEVICE_FIND);

impl Drop for BluetoothFindHandle {
    fn drop(&mut self) {
        // SAFETY:
        // This search handle was returned successfully by
        // BluetoothFindFirstDevice and is owned exclusively by this wrapper.
        unsafe {
            BluetoothFindDeviceClose(self.0);
        }
    }
}

pub(crate) fn enumerate() -> io::Result<Vec<BluetoothDevice>> {
    let search_params = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnAuthenticated: 1,
        fReturnRemembered: 1,
        fReturnUnknown: 0,
        fReturnConnected: 1,
        fIssueInquiry: 0,
        cTimeoutMultiplier: 0,
        hRadio: std::ptr::null_mut(),
    };

    let mut info = empty_device_info();

    // SAFETY:
    // Both structures have their required dwSize fields initialized.
    // hRadio is null, so Windows searches all local Bluetooth radios.
    // fIssueInquiry is false, so this enumerates known devices without
    // performing an active Bluetooth inquiry.
    let raw_find_handle = unsafe { BluetoothFindFirstDevice(&search_params, &mut info) };

    if raw_find_handle.is_null() {
        // SAFETY:
        // GetLastError reads the calling thread's most recent Win32 error.
        let error = unsafe { GetLastError() };

        if error == ERROR_NO_MORE_ITEMS {
            return Ok(Vec::new());
        }

        return Err(io::Error::from_raw_os_error(error as i32));
    }

    let find_handle = BluetoothFindHandle(raw_find_handle);
    let mut devices = Vec::new();

    loop {
        devices.push(device_from_info(&info));

        info = empty_device_info();

        // SAFETY:
        // find_handle is a live Bluetooth enumeration handle and info has
        // its required dwSize field initialized.
        let found = unsafe { BluetoothFindNextDevice(find_handle.0, &mut info) };

        if found != 0 {
            continue;
        }

        // SAFETY:
        // GetLastError reads the calling thread's most recent Win32 error.
        let error = unsafe { GetLastError() };

        if error == ERROR_NO_MORE_ITEMS {
            break;
        }

        return Err(io::Error::from_raw_os_error(error as i32));
    }

    Ok(devices)
}

fn empty_device_info() -> BLUETOOTH_DEVICE_INFO {
    // SAFETY:
    // BLUETOOTH_DEVICE_INFO is a plain Win32 data structure for which
    // zero initialization is valid before setting dwSize.
    let mut info: BLUETOOTH_DEVICE_INFO = unsafe { zeroed() };

    info.dwSize = size_of::<BLUETOOTH_DEVICE_INFO>() as u32;

    info
}

fn device_from_info(info: &BLUETOOTH_DEVICE_INFO) -> BluetoothDevice {
    BluetoothDevice {
        name: wide_string(&info.szName),
        connected: info.fConnected != 0,
        remembered: info.fRemembered != 0,
        authenticated: info.fAuthenticated != 0,
    }
}

fn wide_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());

    String::from_utf16_lossy(&value[..length])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_string_stops_at_nul() {
        let value = [
            'B' as u16, 'a' as u16, 'r' as u16, 'e' as u16, 0, 'X' as u16,
        ];

        assert_eq!(wide_string(&value), "Bare");
    }

    #[test]
    fn wide_string_accepts_full_buffer() {
        let value = ['B' as u16, 'T' as u16];

        assert_eq!(wide_string(&value), "BT");
    }
}
