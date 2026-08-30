# Programming the Pico

This covers flashing the firmware onto the Raspberry Pi Pico and reading its
logs. For wiring the Pico into the iMac's motherboard harness, see
[WIRING.md](WIRING.md) — that's a separate connector from the one used here.

## Prerequisites

- The `thumbv6m-none-eabi` target: `rustup target add thumbv6m-none-eabi`
- [`probe-rs`](https://probe.rs): `cargo install probe-rs-tools --locked`
- An SWD debug probe (e.g. a second Pico running the `picoprobe` firmware, a
  Raspberry Pi Debug Probe, or any other CMSIS-DAP/J-Link/ST-Link probe
  `probe-rs` supports)

`firmware/.cargo/config.toml` already points `cargo run` at `probe-rs run
--chip RP2040`, so no other tooling is needed for the normal flow above. If
you don't have a probe yet, see [Flashing without a debug
probe](#flashing-without-a-debug-probe-bootseluf2) below instead.

## Connecting the debug probe

Wire the probe to the Pico's 3-pin debug port (the castellated pads on the
edge of the board, separate from the GPIO header): probe GND, SWCLK, and
SWDIO to the Pico's GND, SWCLK, and SWDIO respectively. This is independent
of the iMac harness connections in `WIRING.md` — SWD is only needed while
developing/flashing, not for the board's normal operation.

## Build and flash

From `firmware/` (the `.cargo/config.toml` that sets the target and runner
is scoped to this directory, so run these from here, not the repo root):

```sh
cd firmware
cargo run --release
```

This builds the firmware and flashes it over SWD via `probe-rs`, then stays
attached printing `defmt` log output over RTT. Stop it with Ctrl-C; the
firmware keeps running on the Pico after you disconnect.

To only build, without a probe connected:

```sh
cargo build --release
```

## Flashing without a debug probe (BOOTSEL/UF2)

The RP2040 has a built-in USB bootloader, so a probe isn't strictly required
— useful for initial bring-up before you have one wired up. This needs
[`elf2uf2-rs`](https://github.com/JoNil/elf2uf2-rs) (`cargo install
elf2uf2-rs --locked`) instead of `probe-rs`.

Put the Pico in BOOTSEL mode (hold the BOOTSEL button while plugging in USB,
or while pressing reset if the board has a separate reset button) so it
enumerates as a mass-storage drive, then override the runner for one command
via the `CARGO_TARGET_<TRIPLE>_RUNNER` environment variable rather than
editing `.cargo/config.toml` (which would silently switch everyone's default
away from `probe-rs`):

```sh
cd firmware
CARGO_TARGET_THUMBV6M_NONE_EABI_RUNNER="elf2uf2-rs -d" cargo run --release
```

`elf2uf2-rs -d` converts the built ELF to UF2 and deploys it to the first
connected Pico in BOOTSEL mode; the board reboots into the new firmware on
its own. Unlike the `probe-rs` path, there's no `defmt`/RTT log output this
way — fine for confirming the board enumerates and basic USB HID behavior,
but not for anything that needs `smbus.rs`'s scan output or other log lines.
Switch back to a real probe once you need those.

## Logs

Log verbosity is set by `DEFMT_LOG` in `firmware/.cargo/config.toml`
(currently `info`). Override it for one run without editing the file:

```sh
DEFMT_LOG=debug cargo run --release
```

## Troubleshooting

- `probe-rs` reporting no probe found: check the probe's own USB connection
  (separate from the Pico's), and that SWCLK/SWDIO/GND aren't swapped.
- `probe-rs` finds a probe but not the chip: double check GND is actually
  connected — a floating GND is the most common cause of a probe that
  enumerates but can't attach.
- `elf2uf2-rs` reporting no device found: the Pico only shows up while it's
  actually in BOOTSEL mode — if it's already running firmware (including a
  previous flash), you need to hold BOOTSEL and re-plug/reset it again
  first, since booting into normal firmware exits BOOTSEL mode.
