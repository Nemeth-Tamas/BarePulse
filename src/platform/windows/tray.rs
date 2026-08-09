use std::{
    io,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, IDI_APPLICATION, LoadIconW,
            MF_GRAYED, MF_SEPARATOR, MF_STRING, PostMessageW, SetForegroundWindow, TPM_NONOTIFY,
            TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_APP, WM_CONTEXTMENU, WM_NULL,
            WM_RBUTTONUP,
        },
    },
};

use super::wide_null;

const ICON_ID: u32 = 1;
const TOOLTIP: &str = "BarePulse";

const MENU_EXIT_ID: usize = 1;

pub(super) const CALLBACK_MESSAGE: u32 = WM_APP + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    None,
    Exit,
}

pub(super) fn handle_callback(window: HWND, event: isize) -> io::Result<Action> {
    match event as u32 {
        WM_RBUTTONUP | WM_CONTEXTMENU => show_context_menu(window),
        _ => Ok(Action::None),
    }
}

fn show_context_menu(window: HWND) -> io::Result<Action> {
    // SAFETY:
    // CreatePopupMenu creates an empty menu owned by this process.
    let menu = unsafe { CreatePopupMenu() };

    if menu.is_null() {
        return Err(io::Error::last_os_error());
    }

    let result = build_and_show_context_menu(window, menu);

    // SAFETY:
    // menu was created successfully by CreatePopupMenu and is no longer
    // needed after TrackPopupMenuEx has returned.
    unsafe {
        DestroyMenu(menu);
    }

    result
}

fn build_and_show_context_menu(
    window: HWND,
    menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
) -> io::Result<Action> {
    let title = wide_null("BarePulse");
    let exit = wide_null("Exit");

    // SAFETY:
    // menu is a valid popup menu and title is a valid null-terminated
    // UTF-16 string.
    if unsafe { AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, title.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // A separator has no string payload or command identifier.
    if unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, null()) } == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // menu is valid and exit is a valid null-terminated UTF-16 string.
    if unsafe { AppendMenuW(menu, MF_STRING, MENU_EXIT_ID, exit.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // POINT is an output structure populated by GetCursorPos.
    let mut cursor: POINT = unsafe { zeroed() };

    // SAFETY:
    // cursor points to writable POINT storage.
    if unsafe { GetCursorPos(&mut cursor) } == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // Giving our hidden owner window foreground status is the standard
    // notification-area popup-menu pattern.
    unsafe {
        SetForegroundWindow(window);
    }

    // SAFETY:
    // menu and window are valid. TPM_RETURNCMD makes the function return
    // the selected command instead of posting WM_COMMAND.
    let command = unsafe {
        TrackPopupMenuEx(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            cursor.x,
            cursor.y,
            window,
            null(),
        )
    };

    // SAFETY:
    // Posting WM_NULL back to the owner window allows Windows to finish the
    // popup-menu dismissal sequence cleanly.
    unsafe {
        PostMessageW(window, WM_NULL, 0, 0);
    }

    match command as usize {
        MENU_EXIT_ID => Ok(Action::Exit),
        _ => Ok(Action::None),
    }
}

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
