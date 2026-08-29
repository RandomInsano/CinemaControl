# Wiring: RP2040 to the 27" iMac (2009 / A1312) motherboard harness

This board replaces the original logic board's connection into the iMac's
motherboard harness — the 14-pin connector that carries PSU rails, the
PA-2311-02A's SMBus, and backlight control. Pin numbers below are Edwin's own
numbering scheme, traced directly off the harness connector; Apple's silkscreen
numbering on the same connector is rotated 180° from this, so don't cross-
reference Apple service docs by pin number without re-checking orientation
against the keying notch (between pins 3/4 and 10/11 below).

## Harness connector pinout

| Pin | Signal              | Notes                                    |
|-----|---------------------|------------------------------------------|
| 1   | GND                 |                                          |
| 2   | SCL                 | PA-2311-02A SMBus clock                  |
| 3   | HDD V?              | Unidentified, unused by this board       |
| 4   | PP12V_GH3_ACDC      | 12V rail                                 |
| 5   | GND                 |                                          |
| 6   | Backlight PWM       | PWM duty cycle, not the enable line      |
| 7   | GND                 |                                          |
| 8   | GND                 |                                          |
| 9   | SDA                 | PA-2311-02A SMBus data                   |
| 10  | PP12V_S0_PS         | 12V rail                                 |
| 11  | PP12V_S0_PS         | 12V rail                                 |
| 12  | PS_ON               | PSU enable                               |
| 13  | Backlight on (BLON) | Backlight enable, separate from PWM      |
| 14  | HDD V?A             | Unidentified, unused by this board       |

## Signals this firmware drives today

Only three of the fourteen pins are connected to the RP2040; everything else
is grounds and 12V rails that just need to be present on the harness side.
Pin assignments come from `board.rs`, which is the only file that should ever
need to change if this mapping does.

| Harness pin | Signal        | RP2040 | Pico physical pin | `board.rs`     |
|-------------|---------------|--------|-------------------|----------------|
| 2           | SCL           | GPIO5  | 7                 | `SmbusSclPin`  |
| 9           | SDA           | GPIO4  | 6                 | `SmbusSdaPin`  |
| 6           | Backlight PWM | GPIO15 | 20                | `BacklightPin` |
| 1, 5, 7, 8  | GND           | GND    | 3, 8, 13, ...     |                |

SMBus is run at 100kHz (`SMBUS_FREQUENCY_HZ` in `board.rs`) with the RP2040's
I2C0 peripheral in async mode. The RP2040 GPIOs are 3.3V logic — confirm with
a meter that the PA-2311-02A pulls SCL/SDA up to 3.3V (not 5V) on this harness
before connecting, since embassy-rp's I2C pins aren't 5V-tolerant.

### Pico pin diagram

Top view, USB connector at top. Only the three pins this board uses are
called out; every other pin is unconnected.

```
                  ┌───────────────────┐
                  │        USB        │
          GP0   1 │ o               o │ 40  VBUS
          GP1   2 │ o               o │ 39  VSYS
          GND   3 │ o               o │ 38  GND
          GP2   4 │ o               o │ 37  3V3_EN
          GP3   5 │ o               o │ 36  3V3(OUT)
 SDA  ►   GP4   6 │ o               o │ 35  ADC_VREF
 SCL  ►   GP5   7 │ o               o │ 34  GP28
          GND   8 │ o               o │ 33  GND
          GP6   9 │ o               o │ 32  GP27
          GP7  10 │ o               o │ 31  GP26
          GP8  11 │ o               o │ 30  RUN
          GP9  12 │ o               o │ 29  GP22
          GND  13 │ o               o │ 28  GND
         GP10  14 │ o               o │ 27  GP21
         GP11  15 │ o               o │ 26  GP20
         GP12  16 │ o               o │ 25  GP19
         GP13  17 │ o               o │ 24  GP18
          GND  18 │ o               o │ 23  GND
         GP14  19 │ o               o │ 22  GP17
 PWM  ►  GP15  20 │ o               o │ 21  GP16
                  └───────────────────┘
```

SDA/SCL/PWM go to harness pins 9/2/6 per the table above. Any GND pin (3, 8,
13, 18, ...) goes to harness pins 1/5/7/8. VSYS/VBUS (39/40) are the
candidates for powering the Pico itself — see "Powering the RP2040 itself"
below; neither is connected to the harness today.

## Signals not yet connected

**PS_ON (pin 12)** and **Backlight on / BLON (pin 13)** aren't wired to any
GPIO — `board.rs` doesn't allocate pins for them, and no other module drives
them. Without PS_ON asserted the PSU's 12V rails stay off entirely, and
without BLON asserted the backlight stays dark regardless of what the PWM
duty cycle is doing. Until firmware support is added, these need to be
handled outside this board (tied to a fixed level, or driven by something
other than the RP2040) for the display to actually light up.

Pins 3 and 14 (`HDD V?` / `HDD V?A`) are unidentified signals from the
original logic board and aren't used here — leave them unconnected.

## Powering the RP2040 itself

Nothing on this harness currently feeds the RP2040's own supply. During
bring-up/testing, USB VBUS from the host Mac (feeding the Pico's VBUS/VSYS)
is sufficient, since USB is already required for the HID interface. For a
standalone install (RP2040 powered from the iMac's own PSU rather than a
host's USB port), PP12V_GH3_ACDC or PP12V_S0_PS would need to feed a buck
regulator down to VSYS (Pico physical pin 39) — there's no such regulator on
this board today.

## Before connecting

- Confirm common ground between the RP2040 and the harness (pins 1/5/7/8)
  before connecting SCL/SDA/PWM — an SMBus or PWM line without a shared
  ground reference can misbehave or damage the GPIO.
- Confirm the PSU is actually producing SMBus activity before assuming a scan
  found nothing — see `smbus.rs`'s module doc comment: `scan_task` gives the
  bus 2 seconds after boot to settle, but if PS_ON isn't asserted the PSU may
  not be running its SMBus interface at all.
