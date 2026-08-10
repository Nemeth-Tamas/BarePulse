use std::{
    ffi::c_void,
    io,
    mem::{size_of, zeroed},
    slice,
};

use windows_sys::{
    Win32::{
        Devices::HumanInterfaceDevice::HidD_GetHidGuid,
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{RegisterDeviceNotificationW, UnregisterDeviceNotification},
    },
    core::GUID,
};

const DEVICE_NOTIFY_WINDOW_HANDLE: u32 = 0x0000_0000;

const DBT_DEVTYP_DEVICEINTERFACE: u32 = 0x0000_0005;

const DBT_DEVICEARRIVAL: usize = 0x8000;
const DBT_DEVICEREMOVECOMPLETE: usize = 0x8004;

#[repr(C)]
struct DeviceInterfaceFilter {
    size: u32,
    device_type: u32,
    reserved: u32,
    class_guid: GUID,
    name: [u16; 1],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Change {
    Arrival,
    Removal,
}

impl Change {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Arrival => "arrival",
            Self::Removal => "removal",
        }
    }
}

pub(super) fn device_path(l_param: LPARAM) -> Option<String> {
    if l_param == 0 {
        return None;
    }

    let broadcast = l_param as *const DeviceInterfaceFilter;

    // SAFETY:
    // During WM_DEVICECHANGE, l_param points to the broadcast structure
    // supplied by Windows for the duration of the window-procedure call.
    let (size, device_type) = unsafe { ((*broadcast).size as usize, (*broadcast).device_type) };

    if device_type != DBT_DEVTYP_DEVICEINTERFACE {
        return None;
    }

    let name_offset = std::mem::offset_of!(DeviceInterfaceFilter, name);

    if size <= name_offset {
        return None;
    }

    let name_bytes = size - name_offset;

    if name_bytes % size_of::<u16>() != 0 {
        return None;
    }

    let name_units = name_bytes / size_of::<u16>();

    // SAFETY:
    // name_offset points at the variable-length UTF-16 device-interface
    // name stored inside the Windows broadcast structure. name_units is
    // bounded by the byte size supplied in that same structure.
    let name = unsafe {
        let name_ptr = broadcast.cast::<u8>().add(name_offset).cast::<u16>();

        slice::from_raw_parts(name_ptr, name_units)
    };

    decode_device_path(name)
}

fn decode_device_path(name: &[u16]) -> Option<String> {
    let end = name
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(name.len());

    if end == 0 {
        return None;
    }

    String::from_utf16(&name[..end]).ok()
}

pub(super) struct Registration(*mut c_void);

impl Registration {
    pub(super) fn register(window: HWND) -> io::Result<Self> {
        // SAFETY:
        // GUID is a plain Win32 value type where all-zero is a valid
        // temporary value before HidD_GetHidGuid populates it.
        let mut hid_guid: GUID = unsafe { zeroed() };

        // SAFETY:
        // hid_guid points to valid writable GUID storage.
        unsafe {
            HidD_GetHidGuid(&mut hid_guid);
        }

        let filter = DeviceInterfaceFilter {
            size: size_of::<DeviceInterfaceFilter>() as u32,
            device_type: DBT_DEVTYP_DEVICEINTERFACE,
            reserved: 0,
            class_guid: hid_guid,
            name: [0],
        };

        // SAFETY:
        // window is our live hidden window. filter begins with the layout
        // expected for a DEV_BROADCAST_DEVICEINTERFACE_W registration filter
        // and remains alive for the complete call.
        let notification = unsafe {
            RegisterDeviceNotificationW(
                window,
                &filter as *const DeviceInterfaceFilter as *const c_void,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
        };

        if notification.is_null() {
            return Err(io::Error::last_os_error());
        }

        Ok(Self(notification))
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }

        // SAFETY:
        // self.0 was returned by RegisterDeviceNotificationW and has not
        // previously been unregistered.
        unsafe {
            UnregisterDeviceNotification(self.0);
        }
    }
}

pub(super) const fn classify(event: WPARAM) -> Option<Change> {
    match event {
        DBT_DEVICEARRIVAL => Some(Change::Arrival),
        DBT_DEVICEREMOVECOMPLETE => Some(Change::Removal),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_device_arrival() {
        assert_eq!(classify(DBT_DEVICEARRIVAL), Some(Change::Arrival));
    }

    #[test]
    fn classifies_device_removal() {
        assert_eq!(classify(DBT_DEVICEREMOVECOMPLETE), Some(Change::Removal));
    }

    #[test]
    fn ignores_unrelated_device_change() {
        assert_eq!(classify(0), None);
    }

    #[test]
    fn decodes_device_interface_path() {
        let path = r"\\?\HID#VID_1038&PID_1858&MI_03#TEST";

        let mut encoded = path.encode_utf16().collect::<Vec<_>>();

        encoded.push(0);
        encoded.push(0);

        assert_eq!(decode_device_path(&encoded), Some(path.to_string()));
    }
}
