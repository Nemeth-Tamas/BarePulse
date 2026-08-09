# BarePulse

BarePulse is a tiny native Windows hardware-status tray application written in Rust.

Its purpose is simple: expose useful hardware information without requiring large vendor applications, unnecessary background services, embedded browsers, telemetry stacks, or other vendor bloat.

The first supported device is the **SteelSeries Aerox 9 Wireless**, with direct battery monitoring through native Windows APIs.

BarePulse is designed to grow beyond a single mouse while remaining small, portable, targeted, and efficient.

---

## Philosophy

BarePulse is deliberately built around a few rules:

- Windows-native APIs wherever practical
- `windows-sys` instead of higher-level Win32 wrappers
- Minimal dependencies
- Low CPU and memory usage
- Event-driven behavior instead of aggressive polling
- Small, understandable modules
- Shared protocol and transport implementations
- Device-specific data loaded only when needed
- Portable-first design
- No telemetry
- No advertising
- No vendor software required for supported status features
- No unnecessary background services
- No administrator requirement for normal use

If a vendor application is only being kept installed or running to answer something simple like:

> What percentage is my battery?

that is exactly the kind of problem BarePulse is intended to solve.

---

## Current status

BarePulse is under active development.

The current foundation includes:

- Native Win32 resident process
- Native Windows message loop
- Notification-area tray icon
- Tray tooltip
- Clean tray-icon removal path
- Recovery after Windows Explorer / taskbar restarts
- Platform code separated from the application entry point

The next major work is portable configuration followed by native HID discovery and SteelSeries Aerox 9 battery communication.

See `TODO.md` for the current development checklist.

---

## Architecture

BarePulse is intentionally not a monolithic application.

The high-level architecture is:

    Application
        |
        +-- Platform
        |
        +-- Discovery / Registry
        |
        +-- Protocols / Transports
        |
        +-- Device Profiles

Each layer has a specific responsibility.

---

## Application layer

The application layer coordinates BarePulse as a whole.

Responsibilities include:

- Process lifecycle
- Device state
- Refresh scheduling
- Configuration
- Discovery coordination
- Profile management
- Translating hardware state into tray state

The application layer must not contain vendor-specific hardware protocol logic.

---

## Platform layer

Platform-specific Windows functionality lives separately from device logic.

Current and planned responsibilities include:

- Hidden resident Win32 window
- Native message loop
- Notification-area tray icon
- Native context menu
- Windows notifications
- Explorer/taskbar restart recovery
- Device arrival and removal notifications
- Startup registration
- Executable and configuration paths

The platform layer knows how Windows works.

It does not know how a SteelSeries mouse works.

---

## Device architecture

Hardware support follows three layers:

    Device Profile
          |
          v
      Protocol
          |
          v
      Transport

For the SteelSeries Aerox 9:

    SteelSeries Aerox 9 profile
          |
          v
    SteelSeries Aerox/Prime battery protocol
          |
          v
    Windows HID transport

A future Sony Bluetooth headset might instead use:

    Sony WH-1000XM5 profile
          |
          v
    Bluetooth battery/status protocol
          |
          v
    Windows Bluetooth transport

A future Logitech headset may use:

    Logitech headset profile
          |
          v
    Logitech battery protocol
          |
          v
    USB/HID receiver transport

This prevents device-specific files from duplicating shared HID, Bluetooth, or protocol implementations.

---

## Generic device state

The tray UI should not know anything about vendor-specific packet formats.

Devices are translated into generic state such as:

- Device name
- Connection state
- Battery state
- Charging state
- Last-known status

Connection states may include:

- Connected
- Sleeping
- Disconnected

Battery states may include:

- Unknown
- Battery percentage
- Charging percentage

This allows the same tray UI to display SteelSeries, Logitech, Sony, and future hardware without hardcoded vendor-specific menu logic.

---

## Portable-first design

BarePulse is designed to work as a portable application.

The intended layout is:

    BarePulse/
    ├─ BarePulse.exe
    ├─ barepulse.toml
    └─ devices/

The configuration lives beside the executable.

This makes the complete BarePulse installation easy to:

- Copy
- Back up
- Move
- Delete
- Inspect

An installed version should preserve essentially the same layout, likely under:

    %LOCALAPPDATA%\BarePulse\

rather than `Program Files`.

This avoids requiring administrator privileges simply to update configuration or downloaded device profiles.

The portable and installed versions should use the same executable wherever practical.

---

## Hardware discovery

BarePulse performs device discovery:

- At startup
- When Windows reports relevant new hardware
- When the user explicitly requests a refresh or rediscovery

BarePulse should not continuously enumerate the entire machine.

Discovery initially focuses on relevant transports such as:

- USB HID
- Bluetooth later

For HID devices, useful discovery information may include:

- Vendor ID
- Product ID
- Interface number
- Usage page
- Usage
- Product string
- Serial or instance identifier where available

Discovery is not the same as opening hardware.

BarePulse should identify devices first and only send protocol commands to hardware that matches a known supported profile.

Unknown devices must never be blindly poked with vendor-specific commands.

---

## Persistent discovered-device configuration

BarePulse remembers relevant discovered hardware in `barepulse.toml`.

A supported device may conceptually be stored as:

    [[discovered_devices]]
    transport = "usb-hid"
    vendor_id = 0x1038
    product_id = 0x1858
    name = "SteelSeries Aerox 9 Wireless"
    profile = "steelseries.aerox9"
    enabled = true

Unknown relevant hardware may still be recorded:

    [[discovered_devices]]
    transport = "usb-hid"
    vendor_id = 0x1234
    product_id = 0x5678
    name = "Unknown HID device"
    profile = ""
    enabled = false

This makes diagnostics and adding future support much easier.

---

## Targeted device profiles

BarePulse intentionally does not bake every supported device definition into one increasingly large executable.

The GitHub repository acts as a device-profile registry.

Repository profiles are organized approximately as:

    devices/
    ├─ manifest.toml
    ├─ steelseries/
    │  └─ aerox9.toml
    ├─ logitech/
    │  └─ ...
    └─ sony/
       └─ ...

A user's local `devices/` directory contains only profiles required for hardware actually present on that machine.

Someone using only an Aerox 9 should not need to download every Logitech, Sony, Razer, or future BarePulse device definition.

---

## Device registry

BarePulse uses a small manifest to determine whether discovered hardware has a supported profile.

A conceptual registry entry may look like:

    schema = 1

    [[device]]
    id = "steelseries.aerox9"
    transport = "usb-hid"
    vendor_id = 0x1038
    product_ids = [0x1858, 0x185A]
    path = "steelseries/aerox9.toml"
    sha256 = "..."

The startup flow is approximately:

    BarePulse starts
          |
          v
    Discover relevant hardware
          |
          v
    Compare against local configuration
          |
          +-- Known and cached
          |       |
          |       v
          |   Use local profile
          |
          +-- Known but missing profile
          |       |
          |       v
          |   Retrieve required profile
          |
          +-- Unknown
                  |
                  v
            Check registry manifest
                  |
                  +-- Supported
                  |      |
                  |      v
                  |   Download profile
                  |
                  +-- Unsupported
                         |
                         v
                    Record as unknown

Already-cached device profiles must continue working completely offline.

GitHub availability must never be required to monitor hardware that has already been configured successfully.

---

## Profiles are data, not executable plugins

Downloaded device profiles contain configuration data.

They do not contain Rust source code, DLLs, scripts, or other executable plugins.

A profile tells BarePulse things such as:

- Transport type
- Vendor ID
- Product ID
- Interface
- Supported connection modes
- Protocol identifier
- Protocol command parameters

For example, an Aerox 9 profile may conceptually describe:

    id = "steelseries.aerox9"
    name = "SteelSeries Aerox 9 Wireless"

    [[connection]]
    name = "2.4 GHz"
    transport = "hid"
    vendor_id = 0x1038
    product_id = 0x1858
    interface = 3
    battery_protocol = "steelseries-aerox-prime"
    battery_command = 0xD2

    [[connection]]
    name = "wired"
    transport = "hid"
    vendor_id = 0x1038
    product_id = 0x185A
    interface = 3
    battery_protocol = "steelseries-aerox-prime"
    battery_command = 0x92

The actual `steelseries-aerox-prime` protocol implementation exists once inside BarePulse and can be reused by multiple profiles.

A genuinely new protocol may require a new BarePulse executable release.

Native runtime plugins are deliberately not part of the initial design because they would introduce unnecessary ABI, compatibility, security, signing, and executable-download complexity.

---

## Profile integrity

Remote profile updates must not be blindly trusted.

The intended update process is:

    Download temporary profile
          |
          v
    Validate schema
          |
          v
    Calculate SHA-256
          |
          v
    Compare with registry manifest
          |
          +-- Valid
          |     |
          |     v
          |  Atomic replacement
          |
          +-- Invalid
                |
                v
             Discard

A failed or corrupt update must never destroy the last known-working local profile.

Future hardening may include cryptographic signing of the device registry manifest.

---

## Polling and efficiency

BarePulse should remain effectively idle when nothing needs attention.

The target model is approximately:

    Startup
       |
       v
    Discover
       |
       v
    Initial device query
       |
       v
    Sleep
       |
       v
    Periodic status query

Battery status does not need aggressive polling.

A reasonable default target is approximately five minutes, with immediate refresh when:

- BarePulse starts
- The user requests Refresh
- A known device reconnects
- Windows reports relevant device arrival

BarePulse should not:

- Enumerate every device every few seconds
- Reopen working device handles unnecessarily
- Redraw tray state when nothing changed
- Run tight polling loops
- Perform unnecessary network requests

Where practical, device handles should be cached and reopened only after failure or device events.

---

## Sleeping wireless devices

A sleeping wireless device is not necessarily disconnected.

BarePulse should avoid intentionally waking hardware solely to update a battery percentage.

If a known wireless peripheral temporarily stops responding:

- Preserve the last known battery level
- Mark the status as sleeping or stale
- Retry normally later
- Recover immediately after reconnect/activity where possible
- Do not treat one missed packet as a catastrophic error

---

## Tray interface

BarePulse remains primarily a tray application.

The tray icon should eventually display the primary device battery directly.

Possible states include:

    93
    75
    40
    15
    05
    charging
    unknown
    disconnected

The context menu should be generated from generic discovered-device state.

A future menu might look like:

    BarePulse
    ────────────────────
    Aerox 9 Wireless
      Battery       75%
      Connection    2.4 GHz

    WH-1000XM5
      Battery       60%
      Connection    Bluetooth

    Logitech Headset
      Battery       84%
    ────────────────────
    Refresh
    Settings
    Exit

The application must:

- Remove its tray icon cleanly on normal shutdown
- Recover its tray icon after Explorer/taskbar restarts
- Avoid unnecessary tray redraws
- Provide a clean Exit command

---

## Notifications

BarePulse may use native Windows notifications for important status changes such as:

- Low battery
- Critical battery

Notifications must use thresholds and hysteresis to avoid repeated alerts.

Possible initial battery thresholds are:

- 20%
- 10%
- 5%

Exact behavior will be decided during implementation.

---

## First target: SteelSeries Aerox 9 Wireless

Known SteelSeries USB vendor ID:

    0x1038

Aerox 9 Wireless over 2.4 GHz:

    Product ID: 0x1858
    Interface:  3
    Command:    0xD2

Aerox 9 wired:

    Product ID: 0x185A
    Interface:  3
    Command:    0x92

Battery protocol family:

    SteelSeries Aerox / Prime

Known protocol characteristics include:

- Battery commonly represented using a raw 1 through 21 scale
- Converted to approximately 5% increments
- Charging indicated by bit `0x80`
- Multiple HID response layouts may need to be accepted on Windows
- Response command echoes must be validated
- Unrelated or malformed HID packets must be ignored
- Wireless requests require reasonable retry and timeout handling

SteelSeries GG / Engine must not be required for BarePulse battery monitoring.

---

## Future test hardware

After Aerox 9 support is complete and stable, additional hardware available for development includes:

### Logitech wireless headset

A Logitech wireless headset is available for future testing.

The exact model will be recorded when development reaches that device.

The goal is to retrieve battery information without keeping Logitech's large vendor application running solely for battery monitoring.

### Sony WH-1000XM5

A Sony WH-1000XM5 Bluetooth headset is available for testing.

The preferred implementation is native Windows Bluetooth battery/status access wherever possible, without requiring Sony vendor software.

These devices are secondary targets.

Aerox 9 support remains the immediate focus.

---

## Planned source organization

The exact structure may evolve as responsibilities become real, but the intended direction is:

    src/
    ├─ main.rs
    ├─ app.rs
    │
    ├─ config/
    │  ├─ mod.rs
    │  └─ model.rs
    │
    ├─ discovery/
    │  └─ mod.rs
    │
    ├─ devices/
    │  ├─ mod.rs
    │  └─ types.rs
    │
    ├─ protocols/
    │  ├─ mod.rs
    │  └─ steelseries_aerox_prime.rs
    │
    ├─ transports/
    │  ├─ mod.rs
    │  └─ windows_hid.rs
    │
    └─ platform/
       ├─ mod.rs
       └─ windows/
          ├─ mod.rs
          ├─ window.rs
          ├─ tray.rs
          ├─ device_events.rs
          └─ startup.rs

Files should only be created once their responsibility actually exists.

BarePulse should avoid both extremes:

- One enormous monolithic source file
- Hundreds of tiny modules containing only a few trivial lines

---

## Optimization goals

BarePulse should aim for:

- Effectively zero CPU usage while idle
- Tiny memory footprint
- Small executable size
- Minimal thread count
- No tight polling loops
- No repeated unnecessary hardware enumeration
- No unnecessary heap allocation in hot paths
- No unnecessary network traffic
- No network requirement after required profiles are cached

Correctness and robustness take priority over shaving meaningless bytes.

---

## Non-goals

BarePulse is not intended to become:

- RGB management software
- A complete vendor configuration replacement
- A general hardware-control center
- An Electron application
- A telemetry platform
- A permanently running local web server
- A general-purpose hardware benchmark
- A generic downloadable-code/plugin execution host

The focus is lightweight, useful hardware status.

---

## Development workflow

BarePulse is developed using surgical edits.

For each change:

1. Apply the requested edit locally.
2. Format the project.
3. Compile/check.
4. Run tests.
5. Run strict Clippy.
6. Perform runtime or hardware testing when applicable.
7. Commit the complete change.
8. Push to `origin main`.

The pushed GitHub repository is the source of truth for subsequent development work.

Standard quality gate:

    cargo fmt

    cargo check

    cargo test

    cargo clippy `
        --all-targets `
        --all-features `
        -- `
        -D warnings

Standard commit flow:

    git add .

    git commit -m "<message>"

    git push origin main

---

## AI-assisted development

BarePulse is developed with substantial assistance from OpenAI's ChatGPT.

ChatGPT is used to design the architecture and produce much of the implementation code.

The generated code is manually integrated, compiled, tested, reviewed through actual behavior, and validated on real hardware before being accepted into the repository.

AI-generated code is not assumed to be correct simply because it was generated successfully.

Changes are expected to pass formatting, compilation, tests, strict Clippy, and practical hardware testing where applicable.

---

## License

BarePulse is licensed under the MIT License.

See `LICENSE` for details.