use std::{
    io,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
};

use windows_sys::{
    Win32::{
        Devices::{
            Bluetooth::{
                BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS, BluetoothFindDeviceClose,
                BluetoothFindFirstDevice, BluetoothFindNextDevice, HBLUETOOTH_DEVICE_FIND,
            },
            DeviceAndDriverInstallation::{
                DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SP_DEVINFO_DATA,
                SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
                SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW,
            },
            Properties::DEVPROP_TYPE_BYTE,
        },
        Foundation::{
            DEVPROPKEY, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS, ERROR_NOT_FOUND,
            GetLastError, INVALID_HANDLE_VALUE,
        },
    },
    core::GUID,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BluetoothDevice {
    pub(crate) name: String,
    pub(crate) address: u64,
    pub(crate) connected: bool,
    pub(crate) battery_level: Option<u8>,
    pub(crate) battery_instance_id: Option<String>,
    pub(crate) vendor_id_code: Option<u32>,
    pub(crate) product_id: Option<u16>,
    pub(crate) remembered: bool,
    pub(crate) authenticated: bool,
}

struct BluetoothFindHandle(HBLUETOOTH_DEVICE_FIND);

struct DeviceInfoSet(HDEVINFO);

struct BatteryNode {
    instance_id: String,
    level: u8,
    vendor_id_code: Option<u32>,
    product_id: Option<u16>,
}

const BLUETOOTH_ADDRESS_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

const BLUETOOTH_BATTERY_PROPERTY_KEY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104e_a319_6ee2_4701_bd47_8ddb_f425_bbe5),
    pid: 2,
};

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

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        // SAFETY:
        // This device-information-set handle was returned successfully by
        // SetupDiGetClassDevsW and is owned exclusively by this wrapper.
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
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

    let battery_nodes = match enumerate_battery_nodes() {
        Ok(nodes) => nodes,

        Err(_error) => {
            #[cfg(debug_assertions)]
            eprintln!("BarePulse Bluetooth battery discovery failed: {_error}");

            Vec::new()
        }
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
        devices.push(device_from_info(&info, &battery_nodes));

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

fn device_from_info(
    info: &BLUETOOTH_DEVICE_INFO,
    battery_nodes: &[BatteryNode],
) -> BluetoothDevice {
    // SAFETY:
    // BLUETOOTH_ADDRESS is a union. Windows populated this structure and
    // ullLong is the documented integer representation of the same address.
    let address = unsafe { info.Address.Anonymous.ullLong } & BLUETOOTH_ADDRESS_MASK;

    let battery_node = battery_nodes
        .iter()
        .find(|node| instance_matches_address(&node.instance_id, address));

    BluetoothDevice {
        name: wide_string(&info.szName),
        address,
        connected: info.fConnected != 0,
        battery_level: battery_node.map(|node| node.level),
        battery_instance_id: battery_node.map(|node| node.instance_id.clone()),
        vendor_id_code: battery_node.and_then(|node| node.vendor_id_code),
        product_id: battery_node.and_then(|node| node.product_id),
        remembered: info.fRemembered != 0,
        authenticated: info.fAuthenticated != 0,
    }
}

fn enumerate_battery_nodes() -> io::Result<Vec<BatteryNode>> {
    // SAFETY:
    // Null class GUID and enumerator with DIGCF_ALLCLASSES select all local
    // device setup classes. DIGCF_PRESENT restricts the set to devices that
    // Windows currently considers present.
    let raw_device_info_set = unsafe {
        SetupDiGetClassDevsW(null(), null(), null_mut(), DIGCF_ALLCLASSES | DIGCF_PRESENT)
    };

    if raw_device_info_set == INVALID_HANDLE_VALUE as isize {
        return Err(io::Error::last_os_error());
    }

    let device_info_set = DeviceInfoSet(raw_device_info_set);
    let mut nodes = Vec::new();
    let mut index = 0;

    loop {
        // SAFETY:
        // SP_DEVINFO_DATA is an output structure. SetupAPI requires cbSize
        // to be initialized before enumeration.
        let mut device_info_data: SP_DEVINFO_DATA = unsafe { zeroed() };
        device_info_data.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;

        // SAFETY:
        // device_info_set is a live information set and device_info_data
        // points to writable caller-owned storage.
        let found =
            unsafe { SetupDiEnumDeviceInfo(device_info_set.0, index, &mut device_info_data) };

        if found == 0 {
            // SAFETY:
            // Reads the error from the immediately preceding SetupAPI call.
            let error = unsafe { GetLastError() };

            if error == ERROR_NO_MORE_ITEMS {
                break;
            }

            return Err(io::Error::from_raw_os_error(error as i32));
        }

        match read_battery_property(device_info_set.0, &device_info_data) {
            Ok(Some(level)) => match read_instance_id(device_info_set.0, &device_info_data) {
                Ok(Some(instance_id)) => {
                    let vendor_id_code = parse_hex_field(&instance_id, "VID&", 8);

                    let product_id = parse_hex_field(&instance_id, "PID&", 4)
                        .and_then(|value| u16::try_from(value).ok());

                    nodes.push(BatteryNode {
                        instance_id,
                        level,
                        vendor_id_code,
                        product_id,
                    });
                }

                Ok(None) => {}

                Err(_error) => {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "BarePulse Bluetooth battery discovery: \
                             could not read battery-node instance ID: {_error}"
                    );
                }
            },

            Ok(None) => {}

            Err(_error) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "BarePulse Bluetooth battery discovery: \
                     skipping unreadable battery property: {_error}"
                );
            }
        }

        index += 1;
    }

    Ok(nodes)
}

fn read_battery_property(
    device_info_set: HDEVINFO,
    device_info_data: &SP_DEVINFO_DATA,
) -> io::Result<Option<u8>> {
    let mut property_type = 0;
    let mut level = 0u8;

    // SAFETY:
    // device_info_set and device_info_data identify an enumerated PnP device.
    // level is a writable one-byte buffer matching the expected DEVPROP_TYPE_BYTE
    // property. The property key was discovered from Windows' Bluetooth
    // battery metadata exposed on the device instance.
    let result = unsafe {
        SetupDiGetDevicePropertyW(
            device_info_set,
            device_info_data,
            &BLUETOOTH_BATTERY_PROPERTY_KEY,
            &mut property_type,
            &mut level,
            size_of::<u8>() as u32,
            null_mut(),
            0,
        )
    };

    if result == 0 {
        // SAFETY:
        // Reads the error from the immediately preceding SetupAPI call.
        let error = unsafe { GetLastError() };

        if error == ERROR_NOT_FOUND {
            return Ok(None);
        }

        return Err(io::Error::from_raw_os_error(error as i32));
    }

    if property_type != DEVPROP_TYPE_BYTE as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Bluetooth battery property has unexpected type {property_type}"),
        ));
    }

    if level > 100 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Bluetooth battery property is out of range: {level}"),
        ));
    }

    Ok(Some(level))
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
        // Reads the error from the immediately preceding SetupAPI call.
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

    let instance_id = wide_string(&buffer);

    if instance_id.is_empty() {
        return Ok(None);
    }

    Ok(Some(instance_id))
}

fn instance_matches_address(instance_id: &str, address: u64) -> bool {
    let address = format!("{:012X}", address & BLUETOOTH_ADDRESS_MASK);

    instance_id.to_ascii_uppercase().contains(&address)
}

fn parse_hex_field(value: &str, marker: &str, digits: usize) -> Option<u32> {
    let value = value.to_ascii_uppercase();
    let marker = marker.to_ascii_uppercase();

    let start = value.find(&marker)? + marker.len();
    let end = start.checked_add(digits)?;

    let field = value.get(start..end)?;

    if field.len() != digits || !field.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }

    u32::from_str_radix(field, 16).ok()
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

    #[test]
    fn bluetooth_address_matches_pnp_instance_id() {
        let instance_id = r"BTHENUM\{0000111E-0000-1000-8000-00805F9B34FB}_VID&000105D6_PID&000A\B&204E5236&0&00023CCF828A_C00000000";

        assert!(instance_matches_address(instance_id, 0x0002_3CCF_828A));

        assert!(!instance_matches_address(instance_id, 0x0002_3CCF_828B));
    }

    #[test]
    fn bluetooth_pnp_identity_is_parsed() {
        let instance_id = r"BTHENUM\{0000111E-0000-1000-8000-00805F9B34FB}_VID&000105D6_PID&000A\B&204E5236&0&00023CCF828A_C00000000";

        assert_eq!(parse_hex_field(instance_id, "VID&", 8), Some(0x0001_05D6));

        assert_eq!(parse_hex_field(instance_id, "PID&", 4), Some(0x000A));
    }

    #[test]
    fn malformed_bluetooth_pnp_identity_is_rejected() {
        assert_eq!(parse_hex_field("BTHENUM\\VID&HELLO123", "VID&", 8), None);

        assert_eq!(parse_hex_field("BTHENUM\\PID&12", "PID&", 4), None);
    }
}
