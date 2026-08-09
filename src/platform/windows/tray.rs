use std::{
    io,
    mem::{size_of, zeroed},
    ptr::null_mut,
};

use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{IDI_APPLICATION, LoadIconW, WM_APP},
    },
};

const ICON_ID: u32 = 1;
const TOOLTIP: &str = "BarePulse";

pub(super) const CALLBACK_MESSAGE: u32 = WM_APP + 1;

pub(super) fn add(window: HWND) -> io::Result<()> {
    // SAFETY:
    // IDI_APPLICATION is a predefined shared system icon. Passing a null
    // module handle requests the system resource.
    let icon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };

    if icon.is_null() {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // NOTIFYICONDATAW permits zero initialization. We explicitly populate
    // every field required by the flags used below.
    let mut tray_data: NOTIFYICONDATAW = unsafe { zeroed() };

    tray_data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    tray_data.hWnd = window;
    tray_data.uID = ICON_ID;
    tray_data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    tray_data.uCallbackMessage = CALLBACK_MESSAGE;
    tray_data.hIcon = icon;

    copy_wide_to_buffer(TOOLTIP, &mut tray_data.szTip);

    // SAFETY:
    // tray_data contains a valid owner window, icon ID, shared icon handle,
    // callback message, and null-terminated tooltip buffer.
    if unsafe { Shell_NotifyIconW(NIM_ADD, &tray_data) } == 0 {
        return Err(io::Error::other("Shell_NotifyIconW(NIM_ADD) failed"));
    }

    Ok(())
}

pub(super) fn delete(window: HWND) {
    // SAFETY:
    // NOTIFYICONDATAW permits zero initialization. NIM_DELETE only requires
    // the fields used to identify the existing icon.
    let mut tray_data: NOTIFYICONDATAW = unsafe { zeroed() };

    tray_data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    tray_data.hWnd = window;
    tray_data.uID = ICON_ID;

    // SAFETY:
    // The window and icon ID identify the notification-area icon owned by
    // this process. Failure during shutdown requires no further recovery.
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &tray_data);
    }
}

fn copy_wide_to_buffer<const N: usize>(value: &str, buffer: &mut [u16; N]) {
    for (destination, source) in buffer
        .iter_mut()
        .zip(value.encode_utf16().take(N.saturating_sub(1)))
    {
        *destination = source;
    }
}
