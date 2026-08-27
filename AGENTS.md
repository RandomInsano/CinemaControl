# Agent notes for CinemaControl2

## Comments

Keep comments minimal. Don't explain what code does — name things well
instead. The one deliberate exception: HID report descriptors. Raw HID
descriptor bytes (`0x05, 0x80, ...`) are unreadable without a byte-by-byte
gloss of what each item means, so those get commented line-by-line (see
`HID_REPORT_DESCRIPTOR` in `src/hid.rs`). Everywhere else, a comment should
only exist to capture a non-obvious *why* (a hardware quirk, a spec
requirement, a constraint that isn't visible in the code itself).

## Structure

Each peripheral subsystem is its own module (`src/hid.rs`, `src/pwm.rs`,
`src/smbus.rs`, `src/storage.rs`), each with an `init()` that takes
whatever `src/board.rs` gives it for that peripheral and returns whatever's
ready to spawn, plus its `#[embassy_executor::task]` function(s). Where a
peripheral needs more than one `Peri` (a slice + pin, an I2C instance + two
pins, flash + its DMA channel), `board.rs` groups them into a resource
struct (`BacklightResources`, `SmbusResources`, `FlashResources`) and
`init()` takes that struct as its one argument — USB is simple enough to
stay a bare `Peri<'static, UsbPeripheral>`. This means adding or dropping a
pin for some peripheral means editing its resource struct in `board.rs`, not
every call site's parameter list. `board::split()` owns calling
`mcu_hal::init(clock_config())` and handing out the result as a `Board`, so
`src/main.rs` is pure orchestration: `board::split()`, calling each module's
`init()`, spawning tasks. Keep it that way — don't add peripheral setup
directly in `main()`, and don't reach into `mcu_hal::Peripherals` fields
outside `board::split()`.

All interrupt vectors are bound in one place — `board::Irqs` — rather than
per-module, since the PAC-defined vector names (`USBCTRL_IRQ`, `I2C0_IRQ`,
...) are exactly the kind of chip-specific identifier `board.rs` exists to
contain; each module just imports `crate::board::Irqs`.

Keep functions small and single-purpose; prefer several small named
functions over one that mixes concerns (see how `hid::init` delegates to
`usb_builder` / `build_hid_writer` rather than doing all of it inline).
Exception: if the extracted function would be under three lines *and* only
called from one place, don't extract it — leave it inline at the call site
(with a comment above it if it needs a *why*, per the Comments section
above).

`src/hid_tools.rs` is a plain shared-utility module (the `LoadLeBytes` trait
and `Report` builder used by `src/hid.rs` to build report bytes without
manual slice-range math) — not a peripheral subsystem, so it has no
`bind_interrupts!`/`init()`/task shape. Put other non-peripheral, reusable
helpers alongside it rather than growing `hid.rs`/etc. with things that
aren't about the peripheral itself.

## Build

`cargo build --release` targets `thumbv6m-none-eabi` (Raspberry Pi Pico /
RP2040). Build artifacts land in `~/Downloads/CargoBuild`, not
`./target` — that's the user's global `~/.cargo/config.toml`, not something
to override.

## Storage

Brightness persistence uses program flash via the `sequential-storage` crate
(`src/storage.rs`), not hand-rolled wear-leveling. It needs the last two
flash erase sectors reserved in `memory.x`. Unlike the STM32F103C8 this
project previously targeted, `embassy-rp`'s async `Flash` implements
`embedded-storage-async` directly (given a DMA channel + `Async` mode), so no
blocking-to-async adapter is needed.

## SMBus / PA-2311-02A

The PSU's register map is undocumented. `src/smbus.rs` is a **read-only**
diagnostic scanner, not a real driver — no writes to the PSU. Don't add
writes until we have real bus captures confirming what's safe to send.

`src/hid.rs`'s second HID interface (Voltage/Current/Temperature,
`PSU_REPORT_DESCRIPTOR`) is the placeholder wire format for that telemetry —
`PSU_VOLTAGE_MV` / `PSU_CURRENT_MA` / `PSU_TEMPERATURE_DECIC` are `pub` so
that once `smbus.rs` can actually parse a PMBus reply, it can just `.store()`
into them directly; nothing populates them yet. The descriptor deliberately
uses HID Power Device Usage 0x05 "PowerSupply", not 0x04 "UPS" — this is
telemetry from an internal PSU, not a battery-backup device, and tagging it
UPS could make a host treat it like one (e.g. offer battery-loss shutdown
behavior).

## Research

When trying to investigate how a particlar crate works, use `cargo add` and build the documentation with `cargo build` to use a local version of the documentation instead of fetching from the web.