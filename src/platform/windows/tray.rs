use std::{
    io,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
            Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CreateIcon, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
            MF_GRAYED, MF_SEPARATOR, MF_STRING, PostMessageW, SetForegroundWindow, TPM_NONOTIFY,
            TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_APP, WM_CONTEXTMENU, WM_NULL,
            WM_RBUTTONUP,
        },
    },
};

use crate::devices::{BatteryState, ConnectionMode, ConnectionState, DeviceStatus};

use super::wide_null;

const ICON_ID: u32 = 1;

const STATUS_ICON_WIDTH: usize = 16;
const STATUS_ICON_HEIGHT: usize = 16;
const STATUS_ICON_ROW_BYTES: usize = 2;
const STATUS_ICON_MASK_BYTES: usize = STATUS_ICON_HEIGHT * STATUS_ICON_ROW_BYTES;

const MENU_REFRESH_ID: usize = 1;
const MENU_EXIT_ID: usize = 2;

pub(super) const CALLBACK_MESSAGE: u32 = WM_APP + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    None,
    Refresh,
    Exit,
}

pub(super) fn handle_callback(
    window: HWND,
    event: isize,
    statuses: &[DeviceStatus],
) -> io::Result<Action> {
    match event as u32 {
        WM_RBUTTONUP | WM_CONTEXTMENU => show_context_menu(window, statuses),
        _ => Ok(Action::None),
    }
}

fn show_context_menu(window: HWND, statuses: &[DeviceStatus]) -> io::Result<Action> {
    // SAFETY:
    // CreatePopupMenu creates an empty menu owned by this process.
    let menu = unsafe { CreatePopupMenu() };

    if menu.is_null() {
        return Err(io::Error::last_os_error());
    }

    let result = build_and_show_context_menu(window, menu, statuses);

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
    statuses: &[DeviceStatus],
) -> io::Result<Action> {
    append_disabled_item(menu, "BarePulse")?;

    if statuses.is_empty() {
        append_disabled_item(menu, "No supported devices")?;
    } else {
        for status in statuses {
            append_disabled_item(menu, &status.name)?;
            append_disabled_item(menu, &format_status(status))?;
        }
    }

    // SAFETY:
    // A separator has no string payload or command identifier.
    if unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, null()) } == 0 {
        return Err(io::Error::last_os_error());
    }

    append_command_item(menu, MENU_REFRESH_ID, "Refresh")?;
    append_command_item(menu, MENU_EXIT_ID, "Exit")?;

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
        MENU_REFRESH_ID => Ok(Action::Refresh),
        MENU_EXIT_ID => Ok(Action::Exit),
        _ => Ok(Action::None),
    }
}

fn append_disabled_item(
    menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
    value: &str,
) -> io::Result<()> {
    let value = wide_null(value);

    // SAFETY:
    // menu is valid and value is a null-terminated UTF-16 string.
    if unsafe { AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, value.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

fn append_command_item(
    menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
    command: usize,
    value: &str,
) -> io::Result<()> {
    let value = wide_null(value);

    // SAFETY:
    // menu is valid and value is a null-terminated UTF-16 string.
    if unsafe { AppendMenuW(menu, MF_STRING, command, value.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

pub(super) fn add(window: HWND, statuses: &[DeviceStatus]) -> io::Result<()> {
    let icon = create_status_icon(statuses)?;

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

    copy_wide_to_buffer(&format_tooltip(statuses), &mut tray_data.szTip);

    // SAFETY:
    // tray_data contains a valid owner window, icon ID, custom icon handle,
    // callback message, and null-terminated tooltip buffer.
    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &tray_data) };

    // SAFETY:
    // icon was created by CreateIcon. Shell_NotifyIcon has consumed the icon
    // data needed for this notification-area update.
    unsafe {
        DestroyIcon(icon);
    }

    if added == 0 {
        return Err(io::Error::other("Shell_NotifyIconW(NIM_ADD) failed"));
    }

    Ok(())
}

pub(super) fn update(window: HWND, statuses: &[DeviceStatus]) -> io::Result<()> {
    let icon = create_status_icon(statuses)?;

    // SAFETY:
    // NOTIFYICONDATAW permits zero initialization. NIM_MODIFY only needs
    // the icon identity and fields selected by uFlags.
    let mut tray_data: NOTIFYICONDATAW = unsafe { zeroed() };

    tray_data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    tray_data.hWnd = window;
    tray_data.uID = ICON_ID;
    tray_data.uFlags = NIF_ICON | NIF_TIP;
    tray_data.hIcon = icon;

    copy_wide_to_buffer(&format_tooltip(statuses), &mut tray_data.szTip);

    // SAFETY:
    // The window and icon ID identify our existing notification-area icon.
    let modified = unsafe { Shell_NotifyIconW(NIM_MODIFY, &tray_data) };

    // SAFETY:
    // icon was created by CreateIcon and is no longer needed after the
    // notification-area update has consumed it.
    unsafe {
        DestroyIcon(icon);
    }

    if modified == 0 {
        return Err(io::Error::other("Shell_NotifyIconW(NIM_MODIFY) failed"));
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

fn create_status_icon(statuses: &[DeviceStatus]) -> io::Result<*mut core::ffi::c_void> {
    let (and_mask, xor_mask) = render_status_icon(statuses.first());

    // SAFETY:
    // Both masks contain exactly sixteen word-aligned 1-bit scan lines for
    // the requested 16x16 monochrome icon.
    let icon = unsafe {
        CreateIcon(
            null_mut(),
            STATUS_ICON_WIDTH as i32,
            STATUS_ICON_HEIGHT as i32,
            1,
            1,
            and_mask.as_ptr(),
            xor_mask.as_ptr(),
        )
    };

    if icon.is_null() {
        return Err(io::Error::last_os_error());
    }

    Ok(icon)
}

fn render_status_icon(
    status: Option<&DeviceStatus>,
) -> ([u8; STATUS_ICON_MASK_BYTES], [u8; STATUS_ICON_MASK_BYTES]) {
    let mut xor_mask = [0u8; STATUS_ICON_MASK_BYTES];

    draw_battery_outline(&mut xor_mask);

    match status {
        Some(status) => {
            match status.battery {
                BatteryState::Level(level) | BatteryState::Charging(level) => {
                    draw_battery_level(&mut xor_mask, level);
                }

                BatteryState::Unknown => {}
            }

            match status.connection {
                ConnectionState::Disconnected => {
                    draw_disconnected_mark(&mut xor_mask);
                }

                ConnectionState::Sleeping => {
                    draw_sleeping_mark(&mut xor_mask);
                }

                ConnectionState::Connected => match status.battery {
                    BatteryState::Charging(_) => {
                        draw_charging_mark(&mut xor_mask);
                    }

                    BatteryState::Unknown => {
                        draw_unknown_mark(&mut xor_mask);
                    }

                    BatteryState::Level(_) => {}
                },
            }
        }

        None => {
            draw_unknown_mark(&mut xor_mask);
        }
    }

    // Monochrome icon truth table:
    //
    // AND=1/XOR=0 -> transparent / preserve screen
    // AND=0/XOR=1 -> opaque white
    //
    // Make every pixel drawn into xor_mask opaque instead of relying on
    // reverse-screen behavior, which is not useful in the modern tray.
    let mut and_mask = [0xFF; STATUS_ICON_MASK_BYTES];

    for (and_byte, xor_byte) in and_mask.iter_mut().zip(xor_mask.iter()) {
        *and_byte = !*xor_byte;
    }

    (and_mask, xor_mask)
}

fn draw_battery_outline(bits: &mut [u8; STATUS_ICON_MASK_BYTES]) {
    draw_horizontal(bits, 1, 12, 3);
    draw_horizontal(bits, 1, 12, 12);

    draw_vertical(bits, 1, 3, 12);
    draw_vertical(bits, 12, 3, 12);

    draw_vertical(bits, 13, 6, 9);
    draw_vertical(bits, 14, 7, 8);
}

fn draw_battery_level(bits: &mut [u8; STATUS_ICON_MASK_BYTES], level: u8) {
    const INTERIOR_COLUMNS: usize = 8;

    let columns = (usize::from(level.min(100)) * INTERIOR_COLUMNS).div_ceil(100);

    for x in 3..(3 + columns) {
        for y in 5..=10 {
            set_icon_pixel(bits, x, y);
        }
    }
}

fn draw_charging_mark(bits: &mut [u8; STATUS_ICON_MASK_BYTES]) {
    toggle_icon_pixel(bits, 8, 4);
    toggle_icon_pixel(bits, 7, 5);
    toggle_icon_pixel(bits, 7, 6);
    toggle_icon_pixel(bits, 6, 7);
    toggle_icon_pixel(bits, 9, 7);
    toggle_icon_pixel(bits, 8, 8);
    toggle_icon_pixel(bits, 8, 9);
    toggle_icon_pixel(bits, 7, 10);
    toggle_icon_pixel(bits, 6, 11);
}

fn draw_sleeping_mark(bits: &mut [u8; STATUS_ICON_MASK_BYTES]) {
    for x in 5..=9 {
        toggle_icon_pixel(bits, x, 5);
        toggle_icon_pixel(bits, x, 10);
    }

    toggle_icon_pixel(bits, 9, 6);
    toggle_icon_pixel(bits, 8, 7);
    toggle_icon_pixel(bits, 7, 8);
    toggle_icon_pixel(bits, 6, 9);
}

fn draw_disconnected_mark(bits: &mut [u8; STATUS_ICON_MASK_BYTES]) {
    for offset in 0..=5 {
        toggle_icon_pixel(bits, 5 + offset, 5 + offset);
        toggle_icon_pixel(bits, 10 - offset, 5 + offset);
    }
}

fn draw_unknown_mark(bits: &mut [u8; STATUS_ICON_MASK_BYTES]) {
    toggle_icon_pixel(bits, 6, 5);
    toggle_icon_pixel(bits, 7, 4);
    toggle_icon_pixel(bits, 8, 4);
    toggle_icon_pixel(bits, 9, 5);
    toggle_icon_pixel(bits, 9, 6);
    toggle_icon_pixel(bits, 8, 7);
    toggle_icon_pixel(bits, 7, 8);
    toggle_icon_pixel(bits, 7, 10);
}

fn draw_horizontal(
    bits: &mut [u8; STATUS_ICON_MASK_BYTES],
    start_x: usize,
    end_x: usize,
    y: usize,
) {
    for x in start_x..=end_x {
        set_icon_pixel(bits, x, y);
    }
}

fn draw_vertical(bits: &mut [u8; STATUS_ICON_MASK_BYTES], x: usize, start_y: usize, end_y: usize) {
    for y in start_y..=end_y {
        set_icon_pixel(bits, x, y);
    }
}

fn set_icon_pixel(bits: &mut [u8; STATUS_ICON_MASK_BYTES], x: usize, y: usize) {
    if x >= STATUS_ICON_WIDTH || y >= STATUS_ICON_HEIGHT {
        return;
    }

    let byte_index = y * STATUS_ICON_ROW_BYTES + x / 8;
    let bit = 0x80 >> (x % 8);

    bits[byte_index] |= bit;
}

fn toggle_icon_pixel(bits: &mut [u8; STATUS_ICON_MASK_BYTES], x: usize, y: usize) {
    if x >= STATUS_ICON_WIDTH || y >= STATUS_ICON_HEIGHT {
        return;
    }

    let byte_index = y * STATUS_ICON_ROW_BYTES + x / 8;
    let bit = 0x80 >> (x % 8);

    bits[byte_index] ^= bit;
}

fn format_tooltip(statuses: &[DeviceStatus]) -> String {
    match statuses.first() {
        Some(status) => {
            format!("BarePulse - {} - {}", status.name, format_status(status))
        }

        None => "BarePulse - No supported devices".to_string(),
    }
}

fn format_status(status: &DeviceStatus) -> String {
    let mode = match status.mode {
        ConnectionMode::Wired => "Wired",
        ConnectionMode::Wireless => "Wireless",
    };

    let battery = match status.battery {
        BatteryState::Unknown => "Battery unknown".to_string(),
        BatteryState::Level(level) => format!("{level}%"),
        BatteryState::Charging(level) => format!("{level}% - Charging"),
    };

    match status.connection {
        ConnectionState::Connected => format!("{mode} - {battery}"),

        ConnectionState::Sleeping => {
            format!("{mode} - Sleeping - {battery} (last known)")
        }

        ConnectionState::Disconnected => {
            format!("{mode} - Disconnected - {battery} (last known)")
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn status(connection: ConnectionState, battery: BatteryState) -> DeviceStatus {
        DeviceStatus {
            name: "Test device".to_string(),
            mode: ConnectionMode::Wireless,
            connection,
            battery,
        }
    }

    #[test]
    fn battery_level_changes_icon_bits() {
        let low = status(ConnectionState::Connected, BatteryState::Level(10));

        let full = status(ConnectionState::Connected, BatteryState::Level(100));

        let (_, low_bits) = render_status_icon(Some(&low));
        let (_, full_bits) = render_status_icon(Some(&full));

        assert_ne!(low_bits, full_bits);
    }

    #[test]
    fn charging_changes_icon_bits() {
        let normal = status(ConnectionState::Connected, BatteryState::Level(50));

        let charging = status(ConnectionState::Connected, BatteryState::Charging(50));

        let (_, normal_bits) = render_status_icon(Some(&normal));
        let (_, charging_bits) = render_status_icon(Some(&charging));

        assert_ne!(normal_bits, charging_bits);
    }

    #[test]
    fn sleeping_and_disconnected_have_distinct_icons() {
        let sleeping = status(ConnectionState::Sleeping, BatteryState::Level(50));

        let disconnected = status(ConnectionState::Disconnected, BatteryState::Level(50));

        let (_, sleeping_bits) = render_status_icon(Some(&sleeping));
        let (_, disconnected_bits) = render_status_icon(Some(&disconnected));

        assert_ne!(sleeping_bits, disconnected_bits);
    }

    #[test]
    fn missing_device_has_a_visible_icon() {
        let (_, bits) = render_status_icon(None);

        assert!(bits.iter().any(|byte| *byte != 0));
    }
}
