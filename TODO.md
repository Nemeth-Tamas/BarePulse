# BarePulse TODO

## Milestone 0 — Native tray foundation

- [x] Create Rust project
- [x] Use `windows-sys`
- [x] Create native hidden Win32 window
- [x] Create native Win32 message loop
- [x] Add notification-area tray icon
- [x] Add tray tooltip
- [x] Add clean tray-icon deletion path
- [x] Recover tray icon after Explorer/taskbar restart
- [x] Split platform code out of `main.rs`
- [X] Add native tray context menu
- [X] Add clean Exit command

## Milestone 1 — Portable configuration

- [X] Determine executable directory
- [X] Create `barepulse.toml` beside executable
- [X] Load existing configuration
- [X] Add configuration schema/version
- [X] Add atomic configuration writes
- [X] Add basic application settings
- [X] Add discovered-device persistence
- [X] Preserve portable-first layout

## Milestone 2 — Windows HID discovery

- [X] Add raw SetupAPI/HID discovery using `windows-sys`
- [X] Enumerate HID interfaces
- [X] Collect vendor ID
- [X] Collect product ID
- [X] Collect interface number
- [X] Collect usage page
- [X] Collect usage
- [X] Collect product string
- [X] Collect serial/instance identifier where available
- [X] Create generic discovered-hardware model
- [X] Perform discovery at startup
- [X] Persist discovered devices
- [X] Identify Aerox 9 automatically
- [X] Never blindly open/query unknown hardware

## Milestone 3 — Aerox 9 local support

- [X] Add local Aerox 9 device profile
- [X] Add raw Windows HID transport
- [X] Open correct SteelSeries HID interface
- [X] Implement battery request transport
- [X] Implement SteelSeries Aerox/Prime decoder
- [X] Support wireless PID `0x1858`
- [X] Support wireless command `0xD2`
- [X] Support wired PID `0x185A`
- [X] Support wired command `0x92`
- [X] Decode charging state
- [X] Validate response command echo
- [X] Reject malformed responses
- [X] Ignore unrelated HID packets
- [X] Add wireless retries
- [X] Add sensible HID timeouts
- [X] Add sleeping-device state
- [X] Cache working device handle
- [X] Recover from disconnected/stale handle
- [X] Add decoder unit tests
- [X] Add synthetic/captured packet tests
- [X] Confirm operation without SteelSeries GG/Engine

## Milestone 4 — Real tray device status

- [X] Add generic `DeviceStatus`
- [X] Add generic connection state
- [X] Add generic battery state
- [X] Display real Aerox battery in tooltip
- [X] Display wireless/wired connection state
- [X] Display sleeping/disconnected state
- [X] Build context menu from generic device state
- [X] Add manual Refresh
- [X] Add clean Exit
- [X] Add configurable/default polling interval
- [X] Avoid tray redraw when state is unchanged
- [X] Add dynamic battery tray icon
- [X] Add charging tray state
- [X] Add unknown/disconnected tray states
- [X] Add low-battery notifications
- [X] Add notification hysteresis

## v0.1 acceptance

- [X] Starts without SteelSeries GG/Engine
- [X] Automatically discovers Aerox 9
- [X] Reads real Aerox 9 battery level
- [X] Detects charging state
- [X] Distinguishes wireless and wired operation
- [X] Handles sleeping state
- [X] Handles disconnect/reconnect
- [X] Shows useful battery status in Windows tray
- [ ] Runs with negligible idle CPU usage
- [ ] Requires no administrator privileges
- [ ] Works as a portable application

## Milestone 5 — Native device events

- [X] Register for relevant Windows device notifications
- [X] Detect hardware arrival
- [X] Detect hardware removal
- [X] Perform targeted rediscovery on arrival
- [X] Perform immediate refresh after reconnect
- [X] Avoid full periodic hardware rescans
- [X] Update persistent device configuration when new hardware appears

## Milestone 6 — Device profile registry

- [X] Define device-profile schema
- [X] Create repository `devices/` directory
- [X] Create `devices/manifest.toml`
- [X] Add SteelSeries Aerox 9 registry entry
- [X] Add SteelSeries Aerox 9 profile
- [ ] Retrieve registry manifest from GitHub
- [X] Match discovered hardware against registry
- [ ] Download only profiles needed by detected hardware
- [ ] Cache profiles locally
- [ ] Operate offline using cached profiles
- [ ] Validate downloaded profile schema
- [ ] Add SHA-256 profile verification
- [ ] Download profiles to temporary files
- [ ] Atomically replace verified profiles
- [ ] Retain last-known-good profile after failed update
- [ ] Define registry/profile update policy
- [ ] Consider signed registry manifest later

## Milestone 7 — Portable and installed packaging

- [ ] Finalize portable directory structure
- [ ] Keep config beside executable
- [ ] Keep downloaded profiles beside executable
- [ ] Produce portable release package
- [ ] Design installer
- [ ] Install under `%LOCALAPPDATA%\BarePulse`
- [ ] Avoid administrator requirement
- [ ] Add optional Start Menu shortcut
- [ ] Add optional Start with Windows
- [ ] Use same executable for portable and installed modes where practical
- [ ] Add clean uninstall behavior

## Milestone 8 — Additional hardware

### Logitech wireless headset

- [ ] Record exact Logitech headset model
- [ ] Determine receiver/transport
- [ ] Determine battery protocol
- [ ] Add required generic protocol support
- [ ] Add Logitech device profile
- [ ] Test without Logitech application running

### Sony WH-1000XM5

- [ ] Investigate native Windows Bluetooth battery exposure
- [ ] Add Bluetooth discovery transport
- [ ] Add Bluetooth device state support
- [ ] Add Sony WH-1000XM5 profile if required
- [ ] Read battery level
- [ ] Test reconnect/sleep behavior
- [ ] Confirm operation without Sony vendor software

## Future improvements

- [ ] Start-with-Windows setting
- [ ] Settings UI or tray-based settings
- [ ] Configurable primary tray device
- [ ] Configurable polling intervals
- [ ] Per-device enable/disable
- [ ] Better diagnostics for unsupported devices
- [ ] Export useful discovery information for adding new profiles
- [ ] Profile registry signing
- [ ] Additional HID protocols driven by actual hardware demand
- [ ] Additional Bluetooth devices driven by actual hardware demand