use std::{
    cell::RefCell,
    io,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::atomic::{AtomicU32, Ordering},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer,
        PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetTimer, TranslateMessage,
        WM_CLOSE, WM_DESTROY, WM_DEVICECHANGE, WM_TIMER, WNDCLASSEXW,
    },
};

use crate::devices::{BatteryState, ConnectionState, DeviceStatus};

use super::{device_events, tray, wide_null};

const WINDOW_CLASS_NAME: &str = "BarePulseHiddenWindow";
const TASKBAR_CREATED_MESSAGE_NAME: &str = "TaskbarCreated";

static TASKBAR_CREATED_MESSAGE: AtomicU32 = AtomicU32::new(0);

const STATUS_TIMER_ID: usize = 1;
const DEVICE_CHANGE_TIMER_ID: usize = 2;

const DEVICE_CHANGE_DEBOUNCE_MILLISECONDS: u32 = 150;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RefreshReason {
    StatusOnly,
    HardwareArrival(Vec<String>),
}

impl RefreshReason {
    const fn label(&self) -> &'static str {
        match self {
            Self::StatusOnly => "StatusOnly",
            Self::HardwareArrival(_) => "HardwareArrival",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LowBatteryNotificationState {
    last_threshold: Option<u8>,
}

impl LowBatteryNotificationState {
    fn observe(&mut self, status: &DeviceStatus) -> Option<u8> {
        if status.connection != ConnectionState::Connected {
            return None;
        }

        let level = match status.battery {
            BatteryState::Level(level) => level,

            BatteryState::Charging(level) => {
                if level > 25 {
                    self.last_threshold = None;
                }

                return None;
            }

            BatteryState::Unknown => return None,
        };

        if level > 25 {
            self.last_threshold = None;
            return None;
        }

        let threshold = if level <= 5 {
            5
        } else if level <= 10 {
            10
        } else if level <= 20 {
            20
        } else {
            return None;
        };

        if self
            .last_threshold
            .is_some_and(|previous| threshold >= previous)
        {
            return None;
        }

        self.last_threshold = Some(threshold);

        Some(level)
    }
}

struct WindowState {
    statuses: Vec<DeviceStatus>,
    refresh: Box<dyn FnMut(RefreshReason) -> io::Result<Vec<DeviceStatus>>>,
    low_battery_notifications: Vec<LowBatteryNotificationState>,
    hardware_arrival_pending: bool,
    pending_arrival_paths: Vec<String>,
}

thread_local! {
    static WINDOW_STATE: RefCell<Option<WindowState>> = const { RefCell::new(None) };
}

pub(crate) fn run<F>(
    initial_statuses: Vec<DeviceStatus>,
    poll_interval_seconds: u64,
    refresh: F,
) -> io::Result<()>
where
    F: FnMut(RefreshReason) -> io::Result<Vec<DeviceStatus>> + 'static,
{
    WINDOW_STATE.with(|state| {
        let low_battery_notifications =
            vec![LowBatteryNotificationState::default(); initial_statuses.len()];

        *state.borrow_mut() = Some(WindowState {
            statuses: initial_statuses,
            refresh: Box::new(refresh),
            low_battery_notifications,
            hardware_arrival_pending: false,
            pending_arrival_paths: Vec::new(),
        });
    });

    let result = run_window(poll_interval_seconds);

    WINDOW_STATE.with(|state| {
        state.borrow_mut().take();
    });

    result
}

fn run_window(poll_interval_seconds: u64) -> io::Result<()> {
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

    let _device_notifications = device_events::Registration::register(window)?;

    let statuses = status_snapshot();

    tray::add(window, &statuses)?;

    process_current_low_battery(window)?;

    #[cfg(debug_assertions)]
    if std::env::var_os("BAREPULSE_NOTIFICATION_TEST").is_some()
        && let Some(status) = statuses.first()
    {
        tray::show_low_battery_notification(window, status, 20)?;
    }

    let poll_interval = poll_interval_milliseconds(poll_interval_seconds);

    // SAFETY:
    // window is our live hidden window. A null callback causes WM_TIMER messages
    // to be delivered to its window procedure.
    if unsafe { SetTimer(window, STATUS_TIMER_ID, poll_interval, None) } == 0 {
        tray::delete(window);
        return Err(io::Error::last_os_error());
    }

    message_loop()
}

fn status_snapshot() -> Vec<DeviceStatus> {
    WINDOW_STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .map(|state| state.statuses.clone())
            .unwrap_or_default()
    })
}

fn mark_hardware_arrival_pending(device_path: Option<String>) {
    WINDOW_STATE.with(|state| {
        let mut state = state.borrow_mut();

        let Some(state) = state.as_mut() else {
            return;
        };

        state.hardware_arrival_pending = true;

        if let Some(device_path) = device_path
            && !state
                .pending_arrival_paths
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&device_path))
        {
            state.pending_arrival_paths.push(device_path);
        }
    });
}

fn take_device_change_refresh_reason() -> RefreshReason {
    WINDOW_STATE.with(|state| {
        let mut state = state.borrow_mut();

        let Some(state) = state.as_mut() else {
            return RefreshReason::StatusOnly;
        };

        if !state.hardware_arrival_pending {
            return RefreshReason::StatusOnly;
        }

        state.hardware_arrival_pending = false;

        RefreshReason::HardwareArrival(std::mem::take(&mut state.pending_arrival_paths))
    })
}

fn process_current_low_battery(window: HWND) -> io::Result<()> {
    WINDOW_STATE.with(|state| {
        let mut state = state.borrow_mut();

        let state = state
            .as_mut()
            .ok_or_else(|| io::Error::other("BarePulse window state is unavailable"))?;

        let statuses = state.statuses.clone();

        process_low_battery_notifications(window, &statuses, &mut state.low_battery_notifications)
    })
}

fn process_low_battery_notifications(
    window: HWND,
    statuses: &[DeviceStatus],
    notification_states: &mut Vec<LowBatteryNotificationState>,
) -> io::Result<()> {
    notification_states.resize(statuses.len(), LowBatteryNotificationState::default());

    notification_states.truncate(statuses.len());

    for (status, notification_state) in statuses.iter().zip(notification_states) {
        if let Some(level) = notification_state.observe(status) {
            tray::show_low_battery_notification(window, status, level)?;
        }
    }

    Ok(())
}

fn refresh_status(window: HWND, reason: RefreshReason) -> io::Result<()> {
    WINDOW_STATE.with(|state| {
        let mut state = state.borrow_mut();

        let state = state
            .as_mut()
            .ok_or_else(|| io::Error::other("BarePulse window state is unavailable"))?;

        let refreshed = (state.refresh)(reason)?;

        process_low_battery_notifications(
            window,
            &refreshed,
            &mut state.low_battery_notifications,
        )?;

        if refreshed == state.statuses {
            return Ok(());
        }

        tray::update(window, &refreshed)?;
        state.statuses = refreshed;

        Ok(())
    })
}

fn poll_interval_milliseconds(seconds: u64) -> u32 {
    seconds.saturating_mul(1000).clamp(1, u64::from(u32::MAX)) as u32
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
        let statuses = status_snapshot();
        let _ = tray::add(window, &statuses);
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
            // SAFETY:
            // This timer belongs to our hidden window and no longer needs to fire
            // once the window is being destroyed.
            unsafe {
                KillTimer(window, STATUS_TIMER_ID);
                KillTimer(window, DEVICE_CHANGE_TIMER_ID);
            }

            tray::delete(window);

            // SAFETY:
            // Posting WM_QUIT to the current thread is valid while processing
            // destruction of our resident window.
            unsafe {
                PostQuitMessage(0);
            }

            0
        }

        WM_DEVICECHANGE => {
            if let Some(change) = device_events::classify(w_param) {
                #[cfg(debug_assertions)]
                eprintln!("BarePulse device event: HID {}", change.label());

                if change == device_events::Change::Arrival {
                    let device_path = device_events::device_path(l_param);

                    #[cfg(debug_assertions)]
                    match device_path.as_deref() {
                        Some(path) => {
                            eprintln!("BarePulse device event: arrival path={path}");
                        }

                        None => {
                            eprintln!("BarePulse device event: arrival path unavailable");
                        }
                    }

                    mark_hardware_arrival_pending(device_path);
                }

                // SAFETY:
                // Reusing the same timer ID restarts the short debounce window,
                // coalescing bursts of HID-interface notifications into one refresh.
                let timer = unsafe {
                    SetTimer(
                        window,
                        DEVICE_CHANGE_TIMER_ID,
                        DEVICE_CHANGE_DEBOUNCE_MILLISECONDS,
                        None,
                    )
                };

                if timer == 0 {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "BarePulse device event: failed to arm refresh timer: {}",
                        io::Error::last_os_error()
                    );
                }
            }

            // WM_DEVICECHANGE expects TRUE for handled broadcast notifications.
            1
        }

        WM_TIMER if w_param == DEVICE_CHANGE_TIMER_ID => {
            // SAFETY:
            // This is a one-shot logical debounce. Kill it before performing the
            // potentially slower status refresh.
            unsafe {
                KillTimer(window, DEVICE_CHANGE_TIMER_ID);
            }

            let reason = take_device_change_refresh_reason();

            #[cfg(debug_assertions)]
            eprintln!(
                "BarePulse device event: refreshing device status ({})",
                reason.label()
            );

            if let Err(error) = refresh_status(window, reason) {
                #[cfg(debug_assertions)]
                eprintln!("BarePulse device-event refresh failed: {error}");
            }

            0
        }

        WM_TIMER if w_param == STATUS_TIMER_ID => {
            if let Err(error) = refresh_status(window, RefreshReason::StatusOnly) {
                #[cfg(debug_assertions)]
                eprintln!("BarePulse tray refresh failed: {error}");
            }

            0
        }

        tray::CALLBACK_MESSAGE => {
            let statuses = status_snapshot();

            match tray::handle_callback(window, l_param, &statuses) {
                Ok(tray::Action::Refresh) => {
                    if let Err(error) = refresh_status(window, RefreshReason::StatusOnly) {
                        #[cfg(debug_assertions)]
                        eprintln!("BarePulse manual refresh failed: {error}");
                    }
                }

                Ok(tray::Action::Exit) => {
                    // SAFETY:
                    // window is our valid hidden owner window. Destroying it triggers
                    // WM_DESTROY, which removes the tray icon and terminates the loop.
                    unsafe {
                        DestroyWindow(window);
                    }
                }

                Ok(tray::Action::None) | Err(_) => {}
            }

            0
        }

        _ => {
            // SAFETY:
            // Unhandled messages are forwarded with the exact parameters
            // supplied by Windows.
            unsafe { DefWindowProcW(window, message, w_param, l_param) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{ConnectionMode, DeviceStatus};

    fn status(connection: ConnectionState, battery: BatteryState) -> DeviceStatus {
        DeviceStatus {
            name: "Test device".to_string(),
            mode: ConnectionMode::Wireless,
            connection,
            battery,
        }
    }

    #[test]
    fn low_battery_thresholds_only_notify_once() {
        let mut notification = LowBatteryNotificationState::default();

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(20),)),
            Some(20)
        );

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(20),)),
            None
        );

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(15),)),
            None
        );

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(10),)),
            Some(10)
        );

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(5),)),
            Some(5)
        );
    }

    #[test]
    fn notification_rearms_after_battery_recovers() {
        let mut notification = LowBatteryNotificationState::default();

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(20),)),
            Some(20)
        );

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(24),)),
            None
        );

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(20),)),
            None
        );

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(30),)),
            None
        );

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(20),)),
            Some(20)
        );
    }

    #[test]
    fn charging_and_stale_states_do_not_notify() {
        let mut notification = LowBatteryNotificationState::default();

        assert_eq!(
            notification.observe(&status(
                ConnectionState::Connected,
                BatteryState::Charging(10),
            )),
            None
        );

        assert_eq!(
            notification.observe(&status(ConnectionState::Sleeping, BatteryState::Level(10),)),
            None
        );

        assert_eq!(
            notification.observe(&status(
                ConnectionState::Disconnected,
                BatteryState::Level(10),
            )),
            None
        );

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(10),)),
            Some(10)
        );
    }

    #[test]
    fn charging_above_hysteresis_level_rearms_notifications() {
        let mut notification = LowBatteryNotificationState::default();

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(20),)),
            Some(20)
        );

        assert_eq!(
            notification.observe(&status(
                ConnectionState::Connected,
                BatteryState::Charging(30),
            )),
            None
        );

        assert_eq!(
            notification.observe(&status(ConnectionState::Connected, BatteryState::Level(20),)),
            Some(20)
        );
    }
}
