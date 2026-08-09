# BarePulse

BarePulse is a tiny native Windows hardware-status utility written in Rust.

The goal is simple: expose useful hardware information without requiring large vendor applications, background services, embedded browsers, telemetry stacks, or other unnecessary software.

The first supported device is the **SteelSeries Aerox 9 Wireless**, with direct battery monitoring over the Windows HID APIs.

## Philosophy

BarePulse is deliberately build around a few rules:

* Windows-native APIs wherever practical
* `windows-sys` instead of higher-level Win32 wrappers
* Minimal dependencies
* Low memory and CPU usage
* Event-driven behavior instead of aggressive polling
* Small, understandable modules
* No telemetrey
* No advertising
* No vendor software required
* No unnecessary background services

The long-term goal is to make BarePulse a small extensible tray host for useful hardware and system status information.

## Current status

BarePulse is currently under active development.

The initial foundation consists of a native Win32 resident process and message loop. Tray integration and direct HID communicaiton with the Aerox 9 Wireless are being implemented next.

## AI-assisted development

BarePulse is developed with substantial assistance from OpenAI's ChatGPT.

ChatGPT is used to design the architecture and produce much of the implementation code. The code is manually reviewed, integrated, compiled, tested, and validated on real hardware before being committed to the repository.

AI-generated suggestions are not treated as inherently correct; changes are expected to pass formatting, compilation, tests, Clippy, and practical hardware testing where applicable.

## License

BarePulse is licensed under the MIT Licens.

See [LICENSE](LICENSE) for details.