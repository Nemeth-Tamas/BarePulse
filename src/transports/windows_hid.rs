use std::{
    io,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
};

use windows_sys::{
    Win32::{
        Devices::{
            DeviceAndDriverInstallation::{
                DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO, SP_DEVICE_INTERFACE_DATA,
                SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
                SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW,
                SetupDiGetDeviceInterfaceDetailW,
            },
            HumanInterfaceDevice::HidD_GetHidGuid,
        },
        Foundation::{
            ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS, GetLastError, INVALID_HANDLE_VALUE,
        },
    },
    core::GUID,
};

use crate::discovery::{DiscoveredHardware, Transport};

struct DeviceInfoSet(HDEVINFO);

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        // SAFETY:
        // The handle was returned successfully by SetupDiGetClassDevsW and
        // is owned by this DeviceInfoSet.
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

pub(crate) fn enumerate() -> io::Result<Vec<DiscoveredHardware>> {
    // SAFETY:
    // GUID is a plain output structure initialized by HidD_GetHidGuid.
    let mut hid_guid: GUID = unsafe { zeroed() };

    // SAFETY:
    // hid_guid points to writable GUID storage.
    unsafe {
        HidD_GetHidGuid(&mut hid_guid);
    }

    // SAFETY:
    // hid_guid is the HID device-interface class GUID supplied by Windows.
    // Null enumerator and parent select all currently present local HID
    // interfaces.
    let raw_device_info_set = unsafe {
        SetupDiGetClassDevsW(
            &hid_guid,
            null(),
            null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };

    if raw_device_info_set == INVALID_HANDLE_VALUE as isize {
        return Err(io::Error::last_os_error());
    }

    let device_info_set = DeviceInfoSet(raw_device_info_set);
    let mut devices = Vec::new();
    let mut index = 0;

    loop {
        // SAFETY:
        // SP_DEVICE_INTERFACE_DATA is an output structure. SetupAPI requires
        // cbSize to be initialized before enumeration.
        let mut interface_data: SP_DEVICE_INTERFACE_DATA = unsafe { zeroed() };
        interface_data.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;

        // SAFETY:
        // device_info_set and hid_guid are valid for this enumeration.
        let found = unsafe {
            SetupDiEnumDeviceInterfaces(
                device_info_set.0,
                null(),
                &hid_guid,
                index,
                &mut interface_data,
            )
        };

        if found == 0 {
            // SAFETY:
            // GetLastError reads the calling thread's most recent Win32 error.
            let error = unsafe { GetLastError() };

            if error == ERROR_NO_MORE_ITEMS {
                break;
            }

            return Err(io::Error::from_raw_os_error(error as i32));
        }

        if let Some(device) = inspect_interface(device_info_set.0, &interface_data)? {
            devices.push(device);
        }

        index += 1;
    }

    Ok(devices)
}

fn inspect_interface(
    device_info_set: HDEVINFO,
    interface_data: &SP_DEVICE_INTERFACE_DATA,
) -> io::Result<Option<DiscoveredHardware>> {
    // SAFETY:
    // SP_DEVINFO_DATA is populated by SetupDiGetDeviceInterfaceDetailW.
    // SetupAPI requires cbSize to be initialized by the caller.
    let mut device_info_data: SP_DEVINFO_DATA = unsafe { zeroed() };
    device_info_data.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;

    // We deliberately do not request the device-interface path here.
    //
    // Calling with a null detail buffer causes ERROR_INSUFFICIENT_BUFFER,
    // while SetupAPI still fills device_info_data for the backing PnP device.
    // SAFETY:
    // All supplied structures belong to the current device information set.
    let result = unsafe {
        SetupDiGetDeviceInterfaceDetailW(
            device_info_set,
            interface_data,
            null_mut(),
            0,
            null_mut(),
            &mut device_info_data,
        )
    };

    if result == 0 {
        // SAFETY:
        // Reads the error produced by the immediately preceding SetupAPI call.
        let error = unsafe { GetLastError() };

        if error != ERROR_INSUFFICIENT_BUFFER {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
    }

    let Some(instance_id) = read_instance_id(device_info_set, &device_info_data)? else {
        return Ok(None);
    };

    let vendor_id = parse_hex_field_u16(&instance_id, "vid_");
    let product_id = parse_hex_field_u16(&instance_id, "pid_");
    let interface_number = parse_hex_field_u32(&instance_id, "mi_");

    Ok(Some(DiscoveredHardware {
        transport: Transport::UsbHid,
        hardware_key: instance_id,
        vendor_id,
        product_id,
        interface_number,
    }))
}

fn read_instance_id(
    device_info_set: HDEVINFO,
    device_info_data: &SP_DEVINFO_DATA,
) -> io::Result<Option<String>> {
    let mut required_size = 0;

    // First call obtains the required UTF-16 character count.
    // SAFETY:
    // device_info_set and device_info_data identify an enumerated PnP device.
    let result = unsafe {
        SetupDiGetDeviceInstanceIdW(
            device_info_set,
            device_info_data,
            null_mut(),
            0,
            &mut required_size,
        )
    };

    if result == 0 {
        // SAFETY:
        // Reads the error generated by the immediately preceding SetupAPI call.
        let error = unsafe { GetLastError() };

        if error != ERROR_INSUFFICIENT_BUFFER {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
    }

    if required_size == 0 {
        return Ok(None);
    }

    let mut buffer = vec![0u16; required_size as usize];

    // SAFETY:
    // buffer has the exact character capacity requested by SetupAPI.
    let result = unsafe {
        SetupDiGetDeviceInstanceIdW(
            device_info_set,
            device_info_data,
            buffer.as_mut_ptr(),
            required_size,
            null_mut(),
        )
    };

    if result == 0 {
        return Err(io::Error::last_os_error());
    }

    let length = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());

    if length == 0 {
        return Ok(None);
    }

    Ok(Some(String::from_utf16_lossy(&buffer[..length])))
}

fn parse_hex_field_u16(value: &str, field: &str) -> Option<u16> {
    parse_hex_field(value, field).and_then(|value| u16::try_from(value).ok())
}

fn parse_hex_field_u32(value: &str, field: &str) -> Option<u32> {
    parse_hex_field(value, field)
}

fn parse_hex_field(value: &str, field: &str) -> Option<u32> {
    let lowercase = value.to_ascii_lowercase();
    let start = lowercase.find(field)? + field.len();

    let digits: String = lowercase[start..]
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();

    if digits.is_empty() {
        return None;
    }

    u32::from_str_radix(&digits, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usb_hid_identity_fields() {
        let instance = r"HID\VID_1038&PID_1858&MI_03&COL02\7&1234567&0&0001";

        assert_eq!(parse_hex_field_u16(instance, "vid_"), Some(0x1038));
        assert_eq!(parse_hex_field_u16(instance, "pid_"), Some(0x1858));
        assert_eq!(parse_hex_field_u32(instance, "mi_"), Some(3));
    }

    #[test]
    fn parsing_is_case_insensitive() {
        let instance = r"hid\vid_1038&pid_185a&mi_0f\example";

        assert_eq!(parse_hex_field_u16(instance, "vid_"), Some(0x1038));
        assert_eq!(parse_hex_field_u16(instance, "pid_"), Some(0x185A));
        assert_eq!(parse_hex_field_u32(instance, "mi_"), Some(15));
    }

    #[test]
    fn missing_identity_field_returns_none() {
        let instance = r"HID\SOMETHING_WITHOUT_USB_IDS";

        assert_eq!(parse_hex_field_u16(instance, "vid_"), None);
        assert_eq!(parse_hex_field_u16(instance, "pid_"), None);
        assert_eq!(parse_hex_field_u32(instance, "mi_"), None);
    }
}
