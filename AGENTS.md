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
`src/smbus.rs`, `src/storage.rs`), each owning its own `bind_interrupts!`
block (where relevant), an `init()`
that takes raw `Peri<'static, ...>` peripherals and returns whatever's ready
to spawn, and its `#[embassy_executor::task]` function(s). `src/main.rs` is
orchestration only: clock config, calling each module's `init()`, spawning
tasks. Keep it that way — don't add peripheral setup directly in `main()`.

Keep functions small and single-purpose; prefer several small named
functions over one that mixes concerns (see how `hid::init` delegates to
`usb_builder` / `build_hid_writer` rather than doing all of it inline).
Exception: if the extracted function would be under three lines *and* only
called from one place, don't extract it — leave it inline at the call site
(with a comment above it if it needs a *why*, per the Comments section
above).

## Build

`cargo build --release` targets `thumbv7m-none-eabi` (Blue Pill /
STM32F103C8). Build artifacts land in `~/Downloads/CargoBuild`, not
`./target` — that's the user's global `~/.cargo/config.toml`, not something
to override.

## Storage

This chip (STM32F103C8) has no hardware EEPROM — confirmed via the embassy
build output, which never sets its `eeprom` cfg for this chip — so brightness
persistence uses program flash instead, via the `sequential-storage` crate
(`src/storage.rs`), not hand-rolled wear-leveling. It needs the last two
flash pages reserved in `memory.x`. Its API is async-only and embassy-stm32
has no async flash driver for F1, so the blocking `Flash` is wrapped in
`embassy_embedded_hal::adapter::BlockingAsync`.

## SMBus / PA-2311-02A

The PSU's register map is undocumented. `src/smbus.rs` is a **read-only**
diagnostic scanner, not a real driver — no writes to the PSU. Don't add
writes until we have real bus captures confirming what's safe to send.
