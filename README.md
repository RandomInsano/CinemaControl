# CinemaControl

A replacement logic board for the 27" iMac (2009 / A1312) display, built
around an RP2040 (Raspberry Pi Pico). It plugs into the original 14-pin
motherboard harness in place of the logic board and takes over backlight
PWM control and PSU telemetry, exposing both to the host over USB HID.

This repo is a Rust Cargo workspace with two main crates:

- **`firmware/`** — the `no_std`/`no_main` firmware that runs on the RP2040
  (built on Embassy). It drives the backlight PWM, talks SMBus to the PSU's
  INA219 (voltage/current/power) and EMC1403 (temperature) chips, persists
  brightness to flash, and exposes two USB HID interfaces (brightness, PSU
  telemetry).
- **`cinectl/`** — a host-side CLI (macOS and Linux) that talks to the board
  over USB HID via `hidapi`: list connected boards, get/set brightness, read
  PSU telemetry, or watch for live updates.

Shared between them:

- **`protocol/`** — `no_std` crate with the USB vendor/product IDs, report
  lengths, and the `PowerTelemetry`/`ThermalTelemetry` wire structs both
  sides use.
- **`board-hid/`** — host-only crate with the USB HID transport shared by
  `cinectl` and the macOS menu bar companion (`cinectl-menubar/`): board
  discovery and feature-report read/write on top of `hidapi`.

## Drivers

Standalone `no_std` drivers, generic over `embedded-hal-async`'s `I2c` trait
rather than any particular board's bus type, so they drop onto whatever
transport a caller already has. Both are `firmware` dependencies — used
directly by `firmware/src/smbus.rs` — but live as separate workspace members
under `firmware/`, host-buildable and independently testable.

- **`firmware/ina219/`** — driver for the TI INA219 zero-drift, bidirectional
  current/power monitor on the PSU (voltage, current, power), transcribed
  from datasheet SBOS448. The INA219 exposes only six registers with no
  auto-increment across them and no identification registers, so this crate
  has no `identify`/`probe` equivalent, and current/power read back as zero
  until the driver has been calibrated against the board's shunt resistor.
- **`firmware/emc1403/`** — driver for the Microchip EMC1403/EMC1404 SMBus
  temperature sensor family on the PSU, transcribed from datasheet
  DS20005272A. Register-level accessors are exposed alongside the typed ones
  so registers the driver doesn't model directly are still reachable without
  a second abstraction.

## Getting started

- To flash and run the firmware on a Pico, see
  [firmware/README.md](firmware/README.md).
- For the iMac motherboard harness pinout and wiring, see
  [firmware/WIRING.md](firmware/WIRING.md).
- To build and use the host CLI, see [cinectl/README.md](cinectl/README.md).

Firmware and CLI have separate build setups — see
[AGENTS.md](AGENTS.md) for why `cargo build` from the repo root builds
`cinectl` rather than `firmware`, and other workspace-wide notes.

## License

MIT — see [LICENSE](LICENSE).
