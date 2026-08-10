use std::{
    ffi::c_void,
    io,
    mem::size_of,
    ptr::{null, null_mut},
};

use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts,
};

const USER_AGENT: &str = "BarePulse/0.1";

const RESOLVE_TIMEOUT_MILLISECONDS: i32 = 1_500;
const CONNECT_TIMEOUT_MILLISECONDS: i32 = 1_500;
const SEND_TIMEOUT_MILLISECONDS: i32 = 1_500;
const RECEIVE_TIMEOUT_MILLISECONDS: i32 = 2_000;

const HTTPS_PORT: u16 = 443;
const READ_BUFFER_SIZE: usize = 8 * 1024;

struct InternetHandle(*mut c_void);

impl InternetHandle {
    fn new(handle: *mut c_void) -> io::Result<Self> {
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }

        Ok(Self(handle))
    }
}

impl Drop for InternetHandle {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }

        // SAFETY:
        // self.0 is a WinHTTP handle owned exclusively by this wrapper.
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

pub(crate) fn get_https_text(host: &str, path: &str, maximum_bytes: usize) -> io::Result<String> {
    if maximum_bytes == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HTTPS maximum response size must be greater than zero",
        ));
    }

    if !path.starts_with('/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HTTPS request path must start with '/'",
        ));
    }

    let user_agent = wide_null(USER_AGENT);
    let host = wide_null(host);
    let path = wide_null(path);
    let get = wide_null("GET");

    // SAFETY:
    // user_agent is a valid null-terminated UTF-16 string. Null proxy
    // pointers request the automatic WinHTTP proxy configuration.
    let session = InternetHandle::new(unsafe {
        WinHttpOpen(
            user_agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        )
    })?;

    // SAFETY:
    // session is a valid WinHTTP session handle.
    if unsafe {
        WinHttpSetTimeouts(
            session.0,
            RESOLVE_TIMEOUT_MILLISECONDS,
            CONNECT_TIMEOUT_MILLISECONDS,
            SEND_TIMEOUT_MILLISECONDS,
            RECEIVE_TIMEOUT_MILLISECONDS,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // session is valid and host is a null-terminated UTF-16 hostname.
    let connection =
        InternetHandle::new(unsafe { WinHttpConnect(session.0, host.as_ptr(), HTTPS_PORT, 0) })?;

    // SAFETY:
    // connection is valid. get and path are null-terminated UTF-16 strings.
    // Optional HTTP version, referrer, and accept-type arguments are omitted.
    let request = InternetHandle::new(unsafe {
        WinHttpOpenRequest(
            connection.0,
            get.as_ptr(),
            path.as_ptr(),
            null(),
            null(),
            null(),
            WINHTTP_FLAG_SECURE,
        )
    })?;

    // SAFETY:
    // request is a valid synchronous WinHTTP request. This GET request has no
    // additional headers or request body.
    if unsafe { WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // request was successfully sent and is ready to receive its response.
    if unsafe { WinHttpReceiveResponse(request.0, null_mut()) } == 0 {
        return Err(io::Error::last_os_error());
    }

    require_success_status(request.0)?;

    let body = read_response_body(request.0, maximum_bytes)?;

    String::from_utf8(body).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("HTTPS response is not valid UTF-8: {error}"),
        )
    })
}

fn require_success_status(request: *mut c_void) -> io::Result<()> {
    let mut status_code = 0u32;
    let mut status_size = size_of::<u32>() as u32;

    // SAFETY:
    // status_code points to writable u32 storage and status_size describes
    // exactly that buffer. WINHTTP_QUERY_FLAG_NUMBER requests numeric output.
    if unsafe {
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&mut status_code as *mut u32).cast::<c_void>(),
            &mut status_size,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    if status_code != 200 {
        return Err(io::Error::other(format!(
            "HTTPS request returned HTTP {status_code}"
        )));
    }

    Ok(())
}

fn read_response_body(request: *mut c_void, maximum_bytes: usize) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut buffer = [0u8; READ_BUFFER_SIZE];

    loop {
        let mut bytes_read = 0u32;

        // SAFETY:
        // buffer points to READ_BUFFER_SIZE writable bytes and bytes_read
        // points to writable u32 storage.
        if unsafe {
            WinHttpReadData(
                request,
                buffer.as_mut_ptr().cast::<c_void>(),
                buffer.len() as u32,
                &mut bytes_read,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }

        if bytes_read == 0 {
            break;
        }

        let bytes_read = bytes_read as usize;

        if body.len().saturating_add(bytes_read) > maximum_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("HTTPS response exceeded {maximum_bytes} byte limit"),
            ));
        }

        body.extend_from_slice(&buffer[..bytes_read]);
    }

    Ok(body)
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
