# Agent notes for CinemaControl2

This is a Cargo workspace centered on two crates: `firmware/` (the `no_std`
RP2040 firmware) and `cinectl/` (a host-side CLI, developed on macOS but
also buildable on Linux, that talks to it over USB HID — set/query/watch
brightness and PSU telemetry).
`protocol/` is a small `no_std` crate holding everything both of them need to
agree on: the USB IDs and report lengths (`VENDOR_ID`/`PRODUCT_ID`,
`MAX_BRIGHTNESS`, `*_REPORT_LEN`), the `PowerTelemetry`/`ThermalTelemetry`
structs themselves — including their wire encoding (`to_bytes`/`from_bytes`,
built on the `Report` byte-buffer builder in `protocol/src/hid_tools.rs`) and
`Display` impls (used by `cinectl`; unused but harmless in `firmware`, which
logs via `defmt` instead) — and that `Report` builder itself, for anything
that needs to build a HID report by hand (`hid.rs`'s brightness report,
which has no struct of its own). `firmware/src/smbus.rs` imports these
rather than defining its own copies. Since `protocol` is `no_std` (built for
`firmware` too), it only holds things that don't need `std` — anything
host-only lives in `board-hid/` instead (see below). Everything below this
point is `firmware`-specific unless a section says otherwise.

`board-hid/` is a `std` crate holding the host-side HID transport that
`cinectl` and `cinectl-menubar` both need: `device::discover` (enumerating
connected boards by `protocol::VENDOR_ID`/`PRODUCT_ID` and grouping their
four interfaces by USB serial number into a `Board`), `report` (the
brightness report's byte layout — the `PowerTelemetry`-style structs'
encoding lives in `protocol` since `firmware` needs it too, but brightness's
doesn't), `transport` (`open`/`require_path`/`read_feature`, the
feature-report request/response plumbing on top of `hidapi`), and
`telemetry` (one-shot `read_brightness`/`read_power`/`read_power_thermal`/
`read_processor_thermal`, each a `transport::read_feature` call for one of a
`Board`'s interfaces). Both binaries depend on it rather than keeping their
own copies.

## Comments

Keep comments minimal. Don't explain what code does — name things well
instead. Two narrow exceptions, both kept short:

- HID report descriptors: raw descriptor bytes (`0x05, 0x80, ...`) are
  unreadable without a byte-by-byte gloss of what each item means, so those
  get commented line-by-line (see `HID_REPORT_DESCRIPTOR` in `src/hid.rs`).
- Datasheet facts: a register address, calibration constant, or timing value
  can carry a one-line citation of where it came from (chip, datasheet
  section, measured value) since that can't be recovered by reading the code.

Nothing else gets a comment — not a hardware quirk, not a design rationale,
not a "why this and not that," not a note about what an earlier version did.
If something needs that kind of explaining, it belongs in the commit message,
not the source. A comment that isn't one of the two exceptions above, or that
runs past one line, gets deleted rather than kept.

## Structure (firmware)

`firmware/src/board.rs` owns all hardware bring-up: `board::split()` calls
`mcu_hal::init(clock_config())` and turns every raw peripheral into the
driver abstraction its module actually uses (`UsbDriver`, `Backlight` (a
`Pwm`), `SmbusBus`, `BoardFlash`), bundled into a `Board` — plus
`Board::unique_id: &'static str`, the RP2040's attached flash chip's
factory-programmed 64-bit ID (read via `BoardFlash::blocking_unique_id`,
hex-encoded here into `'static` storage). `hid.rs` uses it as the USB serial
number, so every board is distinguishable without any provisioning step —
but takes it as a plain string rather than anything shaped around flash IDs
specifically (clamping it to what a USB string descriptor can hold in
`usb_device_config`, not to 16 hex characters), so a future chip with a
different ID scheme, or any other source entirely, only ever means changing
`board.rs`. No other module
calls a driver's `new()`, reaches into `mcu_hal::Peripherals` fields, or
sees `mcu_hal::peripherals` types at all — `board.rs` is the only place raw
chip/pin names and `bind_interrupts!`'s PAC-defined vector names
(`USBCTRL_IRQ`, `I2C0_IRQ`, ...) appear, so porting to a different chip or
pinout means editing this file — plus the `embassy-rp` chip feature in
`firmware/Cargo.toml` and the clock config in `main.rs`, both inherently
chip-specific — and nothing else.

Each peripheral subsystem is its own module (`hid.rs`, `pwm.rs`, `smbus.rs`,
`storage.rs`, all under `firmware/src/`) that takes its already-brought-up
driver from `Board` and does only its own domain logic on top: `hid.rs`
builds the USB descriptors/HID interfaces, `pwm.rs` seeds the duty cycle
from `hid::BRIGHTNESS`, `storage.rs` wraps the flash in
`sequential-storage`'s `MapStorage`. `smbus.rs` has nothing left to add on
top, so it has no `init()` at all — `main.rs` spawns
`smbus::telemetry_task(p.smbus)` directly. `board.rs` never reads another
module's state (e.g.
`hid::BRIGHTNESS`) to do this — that's specifically why seeding the
backlight's initial duty cycle stays in `pwm.rs` rather than moving into
`split()` with the rest of the PWM setup. `main.rs` is pure orchestration:
`board::split()`, calling each module's `init()`, spawning tasks. Keep it
that way — don't add peripheral setup directly in `main()`.

Keep functions small and single-purpose; prefer several small named
functions over one that mixes concerns (see how `hid::init` delegates to
`usb_builder` / `build_hid_writer` rather than doing all of it inline).
Exception: if the extracted function would be under three lines *and* only
called from one place, don't extract it — leave it inline at the call site
(with a comment above it if it needs a *why*, per the Comments section
above).

The `Report` builder (building report bytes without manual slice-range math)
lives in `protocol/src/hid_tools.rs`, not `firmware/`, since `protocol`'s own
`PowerTelemetry`/`ThermalTelemetry::to_bytes` need it too — `hid.rs` only
uses it directly for the brightness report, a bare `u16` with no shared
struct of its own. Put other non-peripheral, reusable firmware-only helpers
in a module alongside `hid.rs`/etc. rather than growing those files with
things that aren't about the peripheral itself; anything both `firmware` and
`cinectl` need belongs in `protocol/` instead.

`hid::BRIGHTNESS` and `smbus::PSU_TELEMETRY` are each an
`embassy_sync::watch::Watch` doing double duty as both the current value
(read synchronously via `try_get`) and the change notification (awaited via
a receiver's `changed`) — see the doc comments on those statics. This is why
`hid_report_task`/`psu_report_task`/`pwm::task` push a report or apply a
value only when something actually changes, instead of polling on a
`Timer`.

## Build

Firmware always needs `cd firmware` first: `cargo build --release` there
targets `thumbv6m-none-eabi` (Raspberry Pi Pico / RP2040), using
`firmware/.cargo/config.toml`. That config is scoped to the `firmware/`
directory — cargo doesn't pick it up for `-p`/`--workspace` invocations from
the repo root — so a bare `cargo build`/`check` from the root builds
`cinectl` instead (the workspace's `default-members`), not firmware.

## Storage

Brightness persistence uses program flash via the `sequential-storage` crate
(`firmware/src/storage.rs`), not hand-rolled wear-leveling. It needs the
last two flash erase sectors reserved in `firmware/memory.x`. Unlike the
STM32F103C8 this project previously targeted, `embassy-rp`'s async `Flash`
implements `embedded-storage-async` directly (given a DMA channel + `Async`
mode), so no blocking-to-async adapter is needed.

(The USB serial number is a separate concern — see `board::unique_id` above
— and isn't stored here at all, since it's read fresh from the flash chip's
factory-programmed ID every boot rather than persisted.)

## SMBus / PA-2311-02A

The PSU's own PMBus chip's register map is still undocumented and
`firmware/src/smbus.rs` never touches it — no bus scanning/probing code
lives here anymore (it did, historically, to help identify the two chips
below from real bus captures; that's done, so it was deleted rather than
kept as a read-only diagnostic feature). The module only talks to two
confirmed, identified chips: a TI INA219 (`ina219` crate, real
Voltage/Current/Power, calibrated per the confirmed shunt resistor value in
`INA219_CALIBRATION_RAW`'s doc comment) and a Microchip EMC1403 (`emc1403`
crate, real temperature). Both are read every
`telemetry_task` cycle via `update_telemetry`.

`smbus.rs` also owns `PsuTelemetry` / `PSU_TELEMETRY` — `hid.rs`'s second
HID interface (Voltage/Current/Power/Temperature, `PSU_REPORT_DESCRIPTOR`)
just imports and reports it. The descriptor deliberately uses HID Power
Device Usage 0x05 "PowerSupply", not 0x04 "UPS" — this is telemetry from an
internal PSU, not a battery-backup device, and tagging it UPS could make a
host treat it like one (e.g. offer battery-loss shutdown behavior).

## cinectl (host CLI)

`cinectl/` is a `std` binary crate, developed on macOS but also buildable on
Linux (see `cinectl/README.md` for Linux build prerequisites and the udev
rule needed for non-root device access — `cinectl/99-cinemacontrol.rules`),
that talks to the firmware over USB HID: `cinectl
list|get-brightness|set-brightness|get-psu|watch`, via `hidapi`. The
`hidapi` dependency's `macos-shared-device` feature is scoped to a
`[target.'cfg(target_os = "macos")'.dependencies]` table in
`cinectl/Cargo.toml`, not the main `[dependencies]` table, since it only
affects macOS code paths inside `hidapi` itself. `cinectl` depends on
`protocol/` for the wire structs/IDs and on `board-hid/` for device discovery
and HID transport (see above) — it has no `device.rs` or `report.rs` of its
own. Boards are ordered by USB serial number, which is unique per board out of
the box (see `board::unique_id` above), so no provisioning workflow is needed
here.

`cinectl-menubar` depends on the same `board-hid/` crate for discovery and
transport; its own code is just the menu bar UI on top.

## Research

When trying to investigate how a particlar crate works, use `cargo add` and
build the documentation with `cargo build` to use a local version of the
documentation instead of fetching from the web.