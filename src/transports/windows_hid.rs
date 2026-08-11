use std::{
    ffi::c_void,
    io,
    mem::{size_of, size_of_val, zeroed},
    ptr::{null, null_mut},
    slice,
    time::Duration,
};

use windows_sys::{
    Win32::{
        Devices::{
            DeviceAndDriverInstallation::{
                DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO, SP_DEVICE_INTERFACE_DATA,
                SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA, SetupDiCreateDeviceInfoList,
                SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
                SetupDiGetDeviceInstanceIdW, SetupDiGetDeviceInterfaceDetailW,
                SetupDiOpenDeviceInterfaceW,
            },
            HumanInterfaceDevice::{
                HIDD_ATTRIBUTES, HIDP_CAPS, HidD_FreePreparsedData, HidD_GetAttributes,
                HidD_GetHidGuid, HidD_GetPreparsedData, HidD_GetProductString,
                HidD_GetSerialNumberString, HidP_GetCaps,
            },
        },
        Foundation::{
            CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING,
            ERROR_NO_MORE_ITEMS, ERROR_OPERATION_ABORTED, GENERIC_READ, GENERIC_WRITE,
            GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
            ReadFile, WriteFile,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, GetOverlappedResultEx, OVERLAPPED},
            Threading::CreateEventW,
        },
    },
    core::GUID,
};

use crate::discovery::{DiscoveredHardware, Transport};

struct DeviceInfoSet(HDEVINFO);

const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;
const HID_STRING_CAPACITY: usize = 256;

struct OwnedHandle(HANDLE);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HidReportLengths {
    pub(crate) input: u16,
    pub(crate) output: u16,
    pub(crate) feature: u16,
}

pub(crate) struct HidDevice {
    handle: OwnedHandle,
    report_lengths: HidReportLengths,
}

impl HidDevice {
    pub(crate) fn open(device_path: &str) -> io::Result<Self> {
        Self::open_with_access(device_path, GENERIC_READ | GENERIC_WRITE)
    }

    fn open_with_access(device_path: &str, desired_access: u32) -> io::Result<Self> {
        let handle = open_handle(device_path, desired_access, FILE_FLAG_OVERLAPPED)?;

        let capabilities = read_capabilities(handle.0)?;

        Ok(Self {
            handle,
            report_lengths: HidReportLengths {
                input: capabilities.InputReportByteLength,
                output: capabilities.OutputReportByteLength,
                feature: capabilities.FeatureReportByteLength,
            },
        })
    }

    pub(crate) const fn report_lengths(&self) -> HidReportLengths {
        self.report_lengths
    }

    pub(crate) fn write_report(&self, report: &[u8], timeout: Duration) -> io::Result<()> {
        let expected_length = usize::from(self.report_lengths.output);

        if report.len() != expected_length {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "HID output report length {}; expected {}",
                    report.len(),
                    expected_length
                ),
            ));
        }

        let transferred = write_overlapped(self.handle.0, report, timeout)?;

        if transferred != report.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                format!(
                    "partial HID report write: {} of {} bytes",
                    transferred,
                    report.len()
                ),
            ));
        }

        Ok(())
    }

    pub(crate) fn read_report(&self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let report_length = usize::from(self.report_lengths.input);

        if report_length == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HID input report length is zero",
            ));
        }

        let mut report = vec![0u8; report_length];

        let Some(transferred) = read_overlapped(self.handle.0, &mut report, timeout)? else {
            return Ok(None);
        };

        if transferred == 0 {
            return Ok(None);
        }

        report.truncate(transferred);

        Ok(Some(report))
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY:
        // This Win32 handle is owned exclusively by this wrapper and must be
        // released exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Default)]
struct HidMetadata {
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    usage_page: Option<u16>,
    usage: Option<u16>,
    product_string: Option<String>,
    serial_number: Option<String>,
}

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

        match inspect_interface(device_info_set.0, &interface_data) {
            Ok(Some(device)) => devices.push(device),
            Ok(None) => {}

            Err(_error) => {
                #[cfg(debug_assertions)]
                eprintln!("BarePulse discovery: skipping HID interface {index}: {_error}");
            }
        }

        index += 1;
    }

    Ok(devices)
}

pub(crate) fn inspect_path(device_path: &str) -> io::Result<Option<DiscoveredHardware>> {
    // SAFETY:
    // A null class GUID creates an unrestricted empty local device
    // information set. No parent window is required.
    let raw_device_info_set = unsafe { SetupDiCreateDeviceInfoList(null(), null_mut()) };

    if raw_device_info_set == INVALID_HANDLE_VALUE as isize {
        return Err(io::Error::last_os_error());
    }

    let device_info_set = DeviceInfoSet(raw_device_info_set);

    // SAFETY:
    // SP_DEVICE_INTERFACE_DATA is populated by SetupAPI and requires its
    // cbSize member to be initialized before the call.
    let mut interface_data: SP_DEVICE_INTERFACE_DATA = unsafe { zeroed() };

    interface_data.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;

    let device_path_wide = device_path
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();

    // SAFETY:
    // device_info_set is a valid empty information set,
    // device_path_wide is null-terminated, and interface_data points to
    // writable caller-owned storage.
    if unsafe {
        SetupDiOpenDeviceInterfaceW(
            device_info_set.0,
            device_path_wide.as_ptr(),
            0,
            &mut interface_data,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    inspect_interface(device_info_set.0, &interface_data)
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

    let device_path = read_device_path(device_info_set, interface_data, &mut device_info_data)?;

    let Some(instance_id) = read_instance_id(device_info_set, &device_info_data)? else {
        return Ok(None);
    };

    let parsed_vendor_id = parse_hex_field_u16(&instance_id, "vid_");
    let parsed_product_id = parse_hex_field_u16(&instance_id, "pid_");
    let interface_number = parse_hex_field_u32(&instance_id, "mi_");

    #[cfg(debug_assertions)]
    let metadata = match read_hid_metadata(&device_path) {
        Ok(metadata) => metadata,

        Err(error) => {
            eprintln!("BarePulse discovery: metadata unavailable for {instance_id}: {error}");

            HidMetadata::default()
        }
    };

    #[cfg(not(debug_assertions))]
    let metadata = read_hid_metadata(&device_path).unwrap_or_default();

    Ok(Some(DiscoveredHardware {
        transport: Transport::UsbHid,
        hardware_key: instance_id,
        device_path,
        vendor_id: metadata.vendor_id.or(parsed_vendor_id),
        product_id: metadata.product_id.or(parsed_product_id),
        interface_number,
        usage_page: metadata.usage_page,
        usage: metadata.usage,
        product_string: metadata.product_string,
        serial_number: metadata.serial_number,
    }))
}

fn read_device_path(
    device_info_set: HDEVINFO,
    interface_data: &SP_DEVICE_INTERFACE_DATA,
    device_info_data: &mut SP_DEVINFO_DATA,
) -> io::Result<String> {
    let mut required_size = 0;

    // First call obtains the required variable-length detail-buffer size.
    // It is expected to fail with ERROR_INSUFFICIENT_BUFFER.
    // SAFETY:
    // The device information set and interface data are valid enumeration
    // results. device_info_data has the required cbSize initialized.
    let result = unsafe {
        SetupDiGetDeviceInterfaceDetailW(
            device_info_set,
            interface_data,
            null_mut(),
            0,
            &mut required_size,
            device_info_data,
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

    if required_size < size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SetupAPI returned an undersized HID interface-detail buffer",
        ));
    }

    // Vec<usize> provides sufficient native alignment for the SetupAPI
    // structure while still letting us allocate its variable byte length.
    let storage_elements = (required_size as usize).div_ceil(size_of::<usize>());

    let mut storage = vec![0usize; storage_elements];

    let detail_data = storage
        .as_mut_ptr()
        .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();

    // SAFETY:
    // storage is large enough for required_size bytes and naturally aligned
    // for SP_DEVICE_INTERFACE_DETAIL_DATA_W.
    unsafe {
        (*detail_data).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
    }

    // SAFETY:
    // detail_data points to a writable buffer of at least required_size bytes.
    // SetupAPI will populate the device path and refresh device_info_data.
    let result = unsafe {
        SetupDiGetDeviceInterfaceDetailW(
            device_info_set,
            interface_data,
            detail_data,
            required_size,
            null_mut(),
            device_info_data,
        )
    };

    if result == 0 {
        return Err(io::Error::last_os_error());
    }

    let path_offset = std::mem::offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath);

    let path_bytes = required_size as usize - path_offset;
    let path_capacity = path_bytes / size_of::<u16>();

    // SAFETY:
    // DevicePath begins inside the variable-sized buffer populated by SetupAPI.
    // path_capacity is derived from that buffer's reported byte size.
    let path = unsafe { slice::from_raw_parts((*detail_data).DevicePath.as_ptr(), path_capacity) };

    decode_utf16_buffer(path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "SetupAPI returned an empty HID device path",
        )
    })
}

fn read_hid_metadata(device_path: &str) -> io::Result<HidMetadata> {
    let handle = open_metadata_handle(device_path)?;

    // SAFETY:
    // HIDD_ATTRIBUTES is an output structure whose Size field must be
    // initialized before HidD_GetAttributes.
    let mut attributes: HIDD_ATTRIBUTES = unsafe { zeroed() };
    attributes.Size = size_of::<HIDD_ATTRIBUTES>() as u32;

    let (vendor_id, product_id) = if unsafe { HidD_GetAttributes(handle.0, &mut attributes) } {
        (Some(attributes.VendorID), Some(attributes.ProductID))
    } else {
        (None, None)
    };

    let (usage_page, usage) = read_collection_usage(handle.0).unwrap_or((None, None));

    Ok(HidMetadata {
        vendor_id,
        product_id,
        usage_page,
        usage,
        product_string: read_product_string(handle.0),
        serial_number: read_serial_number(handle.0),
    })
}

fn open_metadata_handle(device_path: &str) -> io::Result<OwnedHandle> {
    open_handle(device_path, 0, 0)
}

fn open_handle(device_path: &str, desired_access: u32, flags: u32) -> io::Result<OwnedHandle> {
    let path = wide_null(device_path);

    // SAFETY:
    // path is a valid null-terminated HID interface path supplied by SetupAPI.
    // The handle is opened with sharing enabled so BarePulse can coexist with
    // other software using the same HID collection.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            desired_access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            flags,
            null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }

    Ok(OwnedHandle(handle))
}

fn write_overlapped(handle: HANDLE, buffer: &[u8], timeout: Duration) -> io::Result<usize> {
    let (event, mut overlapped) = create_overlapped()?;

    // SAFETY:
    // handle was opened for overlapped write access. buffer remains alive
    // until the operation is synchronously completed or canceled below.
    let started = unsafe {
        WriteFile(
            handle,
            buffer.as_ptr(),
            buffer.len() as u32,
            null_mut(),
            &mut overlapped,
        )
    };

    if started == 0 {
        // SAFETY:
        // Reads the error from the immediately preceding WriteFile call.
        let error = unsafe { GetLastError() };

        if error != ERROR_IO_PENDING {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
    }

    let result = complete_overlapped(handle, &mut overlapped, timeout)?;

    drop(event);

    match result {
        Some(transferred) => Ok(transferred as usize),
        None => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "HID report write timed out",
        )),
    }
}

fn read_overlapped(
    handle: HANDLE,
    buffer: &mut [u8],
    timeout: Duration,
) -> io::Result<Option<usize>> {
    let (event, mut overlapped) = create_overlapped()?;

    // SAFETY:
    // handle was opened for overlapped read access. buffer remains alive
    // until the operation is synchronously completed or canceled below.
    let started = unsafe {
        ReadFile(
            handle,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            null_mut(),
            &mut overlapped,
        )
    };

    if started == 0 {
        // SAFETY:
        // Reads the error from the immediately preceding ReadFile call.
        let error = unsafe { GetLastError() };

        if error != ERROR_IO_PENDING {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
    }

    let result = complete_overlapped(handle, &mut overlapped, timeout)?;

    drop(event);

    Ok(result.map(|transferred| transferred as usize))
}

fn create_overlapped() -> io::Result<(OwnedHandle, OVERLAPPED)> {
    // SAFETY:
    // No security descriptor or name is needed. A manual-reset event is used
    // for one overlapped operation.
    let event = unsafe { CreateEventW(null(), 1, 0, null()) };

    if event.is_null() {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // OVERLAPPED permits zero initialization before its event is assigned.
    let mut overlapped: OVERLAPPED = unsafe { zeroed() };
    overlapped.hEvent = event;

    Ok((OwnedHandle(event), overlapped))
}

fn complete_overlapped(
    handle: HANDLE,
    overlapped: &mut OVERLAPPED,
    timeout: Duration,
) -> io::Result<Option<u32>> {
    let mut transferred = 0;

    // SAFETY:
    // overlapped belongs to the pending operation on handle and remains alive
    // for the entire wait/cancellation sequence.
    let completed = unsafe {
        GetOverlappedResultEx(
            handle,
            overlapped,
            &mut transferred,
            timeout_milliseconds(timeout),
            0,
        )
    };

    if completed != 0 {
        return Ok(Some(transferred));
    }

    // SAFETY:
    // Reads the error from GetOverlappedResultEx.
    let error = unsafe { GetLastError() };

    if error != WAIT_TIMEOUT && error != ERROR_IO_INCOMPLETE {
        return Err(io::Error::from_raw_os_error(error as i32));
    }

    // SAFETY:
    // Requests cancellation of this exact pending operation. A completion
    // wait follows before overlapped is allowed to go out of scope.
    unsafe {
        CancelIoEx(handle, overlapped);
    }

    transferred = 0;

    // SAFETY:
    // Waiting here guarantees the canceled/racing operation has finished
    // before its OVERLAPPED structure and buffer are released.
    let completed = unsafe { GetOverlappedResult(handle, overlapped, &mut transferred, 1) };

    if completed != 0 {
        return Ok(Some(transferred));
    }

    // SAFETY:
    // Reads the completion result from GetOverlappedResult.
    let error = unsafe { GetLastError() };

    if error == ERROR_OPERATION_ABORTED {
        return Ok(None);
    }

    Err(io::Error::from_raw_os_error(error as i32))
}

fn timeout_milliseconds(timeout: Duration) -> u32 {
    if timeout.is_zero() {
        return 0;
    }

    timeout.as_millis().clamp(1, u128::from(u32::MAX)) as u32
}

fn read_collection_usage(handle: HANDLE) -> Option<(Option<u16>, Option<u16>)> {
    let capabilities = read_capabilities(handle).ok()?;

    Some((Some(capabilities.UsagePage), Some(capabilities.Usage)))
}

fn read_capabilities(handle: HANDLE) -> io::Result<HIDP_CAPS> {
    let mut preparsed_data = 0isize;

    // SAFETY:
    // handle is an open HID top-level collection. The HID runtime owns the
    // returned preparsed-data allocation until HidD_FreePreparsedData.
    let got_preparsed_data = unsafe { HidD_GetPreparsedData(handle, &mut preparsed_data) };

    if !got_preparsed_data {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // HIDP_CAPS is an output structure populated by HidP_GetCaps.
    let mut capabilities: HIDP_CAPS = unsafe { zeroed() };

    // SAFETY:
    // preparsed_data was returned successfully by HidD_GetPreparsedData.
    let status = unsafe { HidP_GetCaps(preparsed_data, &mut capabilities) };

    // SAFETY:
    // preparsed_data is the allocation returned by HidD_GetPreparsedData and
    // must be released exactly once.
    unsafe {
        HidD_FreePreparsedData(preparsed_data);
    }

    if status != HIDP_STATUS_SUCCESS {
        return Err(io::Error::other(format!(
            "HidP_GetCaps failed with status 0x{:08X}",
            status as u32
        )));
    }

    Ok(capabilities)
}

fn read_product_string(handle: HANDLE) -> Option<String> {
    let mut buffer = [0u16; HID_STRING_CAPACITY];

    // SAFETY:
    // buffer is writable and its byte length is supplied exactly.
    let got_product_string = unsafe {
        HidD_GetProductString(
            handle,
            buffer.as_mut_ptr().cast::<c_void>(),
            size_of_val(&buffer) as u32,
        )
    };

    if !got_product_string {
        return None;
    }

    decode_utf16_buffer(&buffer)
}

fn read_serial_number(handle: HANDLE) -> Option<String> {
    let mut buffer = [0u16; HID_STRING_CAPACITY];

    // SAFETY:
    // buffer is writable and its byte length is supplied exactly.
    let got_serial_number = unsafe {
        HidD_GetSerialNumberString(
            handle,
            buffer.as_mut_ptr().cast::<c_void>(),
            size_of_val(&buffer) as u32,
        )
    };

    if !got_serial_number {
        return None;
    }

    decode_utf16_buffer(&buffer)
}

fn decode_utf16_buffer(buffer: &[u16]) -> Option<String> {
    let length = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());

    if length == 0 {
        return None;
    }

    let value = String::from_utf16_lossy(&buffer[..length]);

    if value.trim().is_empty() {
        return None;
    }

    Some(value)
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
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
    fn decodes_null_terminated_utf16() {
        let buffer = [
            b'A' as u16,
            b'e' as u16,
            b'r' as u16,
            b'o' as u16,
            b'x' as u16,
            0,
            b'X' as u16,
        ];

        assert_eq!(decode_utf16_buffer(&buffer).as_deref(), Some("Aerox"));
    }

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
