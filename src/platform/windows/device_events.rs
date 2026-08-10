use std::{
    ffi::c_void,
    io,
    mem::{size_of, zeroed},
};

use windows_sys::{
    Win32::{
        Devices::HumanInterfaceDevice::HidD_GetHidGuid,
        Foundation::{HWND, WPARAM},
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
}
