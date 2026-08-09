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

- [ ] Add local Aerox 9 device profile
- [X] Add raw Windows HID transport
- [X] Open correct SteelSeries HID interface
- [ ] Implement battery request transport
- [ ] Implement SteelSeries Aerox/Prime decoder
- [ ] Support wireless PID `0x1858`
- [ ] Support wireless command `0xD2`
- [ ] Support wired PID `0x185A`
- [ ] Support wired command `0x92`
- [ ] Decode charging state
- [ ] Validate response command echo
- [ ] Reject malformed responses
- [ ] Ignore unrelated HID packets
- [ ] Add wireless retries
- [ ] Add sensible HID timeouts
- [ ] Add sleeping-device state
- [ ] Cache working device handle
- [ ] Recover from disconnected/stale handle
- [ ] Add decoder unit tests
- [ ] Add synthetic/captured packet tests
- [ ] Confirm operation without SteelSeries GG/Engine

## Milestone 4 — Real tray device status

- [ ] Add generic `DeviceStatus`
- [ ] Add generic connection state
- [ ] Add generic battery state
- [ ] Display real Aerox battery in tooltip
- [ ] Display wireless/wired connection state
- [ ] Display sleeping/disconnected state
- [ ] Build context menu from generic device state
- [ ] Add manual Refresh
- [ ] Add clean Exit
- [ ] Add configurable/default polling interval
- [ ] Avoid tray redraw when state is unchanged
- [ ] Add dynamic battery tray icon
- [ ] Add charging tray state
- [ ] Add unknown/disconnected tray states
- [ ] Add low-battery notifications
- [ ] Add notification hysteresis

## v0.1 acceptance

- [ ] Starts without SteelSeries GG/Engine
- [ ] Automatically discovers Aerox 9
- [ ] Reads real Aerox 9 battery level
- [ ] Detects charging state
- [ ] Distinguishes wireless and wired operation
- [ ] Handles sleeping state
- [ ] Handles disconnect/reconnect
- [ ] Shows useful battery status in Windows tray
- [ ] Runs with negligible idle CPU usage
- [ ] Requires no administrator privileges
- [ ] Works as a portable application

## Milestone 5 — Native device events

- [ ] Register for relevant Windows device notifications
- [ ] Detect hardware arrival
- [ ] Detect hardware removal
- [ ] Perform targeted rediscovery on arrival
- [ ] Perform immediate refresh after reconnect
- [ ] Avoid full periodic hardware rescans
- [ ] Update persistent device configuration when new hardware appears

## Milestone 6 — Device profile registry

- [ ] Define device-profile schema
- [ ] Create repository `devices/` directory
- [ ] Create `devices/manifest.toml`
- [ ] Add SteelSeries Aerox 9 registry entry
- [ ] Add SteelSeries Aerox 9 profile
- [ ] Retrieve registry manifest from GitHub
- [ ] Match discovered hardware against registry
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