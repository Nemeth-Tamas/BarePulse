use std::{
    io,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::atomic::{AtomicU32, Ordering},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
            IDI_APPLICATION, LoadIconW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW,
            TranslateMessage, WM_APP, WM_CLOSE, WM_DESTROY, WNDCLASSEXW,
        },
    },
};

const WINDOW_CLASS_NAME: &str = "BarePulseHiddenWindow";
const TASKBAR_CREATED_MESSAGE_NAME: &str = "TaskbarCreated";
const TRAY_ICON_ID: u32 = 1;
const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 1;
const TRAY_TOOLTIP: &str = "BarePulse";

static TASKBAR_CREATED_MESSAGE: AtomicU32 = AtomicU32::new(0);

fn main() {
    if let Err(error) = run() {
        eprintln!("BarePulse failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let class_name = wide_null(WINDOW_CLASS_NAME);
    let taskbar_created_name = wide_null(TASKBAR_CREATED_MESSAGE_NAME);

    // SAFETY:
    // taskbar_created_name is a valid null-terminated UTF-16 string.
    let taskbar_created_message = unsafe { RegisterWindowMessageW(taskbar_created_name.as_ptr()) };

    if taskbar_created_message == 0 {
        return Err(io::Error::last_os_error());
    }

    TASKBAR_CREATED_MESSAGE.store(taskbar_created_message, Ordering::Relaxed);

    // SAFETY:
    // A null module name requests the module handle for this executable.
    let instance = unsafe { GetModuleHandleW(null()) };

    if instance.is_null() {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // WNDCLASSEXW is a plain Win32 structure where zero is a valid default
    // for the optional handles and class attributes we do not use.
    let mut window_class: WNDCLASSEXW = unsafe { zeroed() };

    window_class.cbSize = size_of::<WNDCLASSEXW>() as u32;
    window_class.lpfnWndProc = Some(window_proc);
    window_class.hInstance = instance;
    window_class.lpszClassName = class_name.as_ptr();

    // SAFETY:
    // window_class points to valid data for the duration of the call and
    // lpszClassName is backed by class_name, which remains alive.
    if unsafe { RegisterClassExW(&window_class) } == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // The registered class name and module handle are valid.
    // No parent, menu, title, or creation parameter is required.
    let window = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };

    if window.is_null() {
        return Err(io::Error::last_os_error());
    }

    add_tray_icon(window)?;

    message_loop()
}

fn message_loop() -> io::Result<()> {
    // SAFETY:
    // MSG is an output structure populated by GetMessageW.
    let mut message = unsafe { zeroed() };

    loop {
        // SAFETY:
        // message points to writable MSG storage. A null HWND receives
        // messages for every window owned by this thread.
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };

        if result == -1 {
            return Err(io::Error::last_os_error());
        }

        if result == 0 {
            return Ok(());
        }

        // SAFETY:
        // message was successfully populated by GetMessageW.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let taskbar_created_message = TASKBAR_CREATED_MESSAGE.load(Ordering::Relaxed);

    if taskbar_created_message != 0 && message == taskbar_created_message {
        let _ = add_tray_icon(window);
        return 0;
    }

    match message {
        WM_CLOSE => {
            // SAFETY:
            // window is the HWND supplied by Windows to this window procedure.
            unsafe {
                DestroyWindow(window);
            }

            0
        }

        WM_DESTROY => {
            delete_tray_icon(window);

            // SAFETY:
            // Posting WM_QUIT to the current thread is valid while processing
            // destruction of our resident window.
            unsafe {
                PostQuitMessage(0);
            }

            0
        }

        TRAY_CALLBACK_MESSAGE => 0,

        _ => {
            // SAFETY:
            // Unhandled messages are forwarded with the exact parameters
            // supplied by Windows.
            unsafe { DefWindowProcW(window, message, w_param, l_param) }
        }
    }
}

fn add_tray_icon(window: HWND) -> io::Result<()> {
    // SAFETY:
    // IDI_APPLICATION is a predefined shared system icon. Passing a null
    // module handle requests the system resource.
    let icon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };

    if icon.is_null() {
        return Err(io::Error::last_os_error());
    }

    // SAFETY:
    // NOTIFYICONDATAW permits zero initialization. We explicitely populate
    // every field required by the flags used below.
    let mut tray_data: NOTIFYICONDATAW = unsafe { zeroed() };

    tray_data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    tray_data.hWnd = window;
    tray_data.uID = TRAY_ICON_ID;
    tray_data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    tray_data.uCallbackMessage = TRAY_CALLBACK_MESSAGE;
    tray_data.hIcon = icon;

    copy_wide_to_buffer(TRAY_TOOLTIP, &mut tray_data.szTip);

    // SAFETY:
    // tray_data contains a valid owner window, icon ID, shared icon handle,
    // callback message, and null-terminated tooltip buffer.
    if unsafe { Shell_NotifyIconW(NIM_ADD, &tray_data) } == 0 {
        return Err(io::Error::other("Shell_NotifyIconW(NIM_ADD) failed"));
    }

    Ok(())
}

fn delete_tray_icon(window: HWND) {
    // SAFETY:
    // NOTIFYICONDATAW permits zero initialization. NIM_DELETE only requires
    // the fields used to identify the existing icon.
    let mut tray_data: NOTIFYICONDATAW = unsafe { zeroed() };

    tray_data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    tray_data.hWnd = window;
    tray_data.uID = TRAY_ICON_ID;

    // SAFETY:
    // The window and icon ID identify the notification-area icon owned by
    // this process. Failure during shutdown required no further recovery.
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

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
