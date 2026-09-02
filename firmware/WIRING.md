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

| Harness pin | Signal        | RP2040 | Pico physical pin | RP2040-Zero pad | `board.rs`     |
|-------------|---------------|--------|--------------------|-----------------|----------------|
| 2           | SCL           | GPIO5  | 7                  | 18              | `SmbusSclPin`  |
| 9           | SDA           | GPIO4  | 6                  | 19              | `SmbusSdaPin`  |
| 6           | Backlight PWM | GPIO15 | 20                 | 8               | `BacklightPin` |
| 1, 5, 7, 8  | GND           | GND    | 3, 8, 13, ...      | 2               |                |

GPIO16 isn't part of the harness at all — on a build with the `neopixel`
feature (see `firmware/README.md`), it drives a WS2812 ("NeoPixel") that
mirrors the current backlight brightness (`neopixel.rs`, toggled off via its
`ENABLED` const): the RP2040-Zero's onboard one if that's the board, or an
external one wired to GPIO16 on a Pico. Unused without that feature.

SMBus is run at 100kHz (`SMBUS_FREQUENCY_HZ` in `board.rs`) with the RP2040's
I2C0 peripheral in async mode. The RP2040 GPIOs are 3.3V logic — confirm with
a meter that the PA-2311-02A pulls SCL/SDA up to 3.3V (not 5V) on this harness
before connecting, since embassy-rp's I2C pins aren't 5V-tolerant.

### Pico pin diagram

Top view, USB connector at top. Only the pins this board uses are called
out (SDA/SCL/PWM go to the harness, FAN/TACH go to the fan connector,
NEOPIXEL is feature-gated and off the harness entirely); every other pin is
unconnected.

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
 FAN  ►   GP6   9 │ o               o │ 32  GP27
          GP7  10 │ o               o │ 31  GP26
          GP8  11 │ o               o │ 30  RUN
 TACH  ►  GP9  12 │ o               o │ 29  GP22
          GND  13 │ o               o │ 28  GND
         GP10  14 │ o               o │ 27  GP21
         GP11  15 │ o               o │ 26  GP20
         GP12  16 │ o               o │ 25  GP19
         GP13  17 │ o               o │ 24  GP18
          GND  18 │ o               o │ 23  GND
         GP14  19 │ o               o │ 22  GP17
 PWM  ►  GP15  20 │ o               o │ 21  GP16  ◄  NEOPIXEL (optional)
                  └───────────────────┘
```

SDA/SCL/PWM go to harness pins 9/2/6 per the table above. Any GND pin (3, 8,
13, 18, ...) goes to harness pins 1/5/7/8. VSYS/VBUS (39/40) are the
candidates for powering the Pico itself — see "Powering the RP2040 itself"
below; neither is connected to the harness today. FAN/TACH go to the fan
connector, not the harness — see the next section. NEOPIXEL (GP16) is only
present with the `neopixel` feature enabled — see the note above the
diagram.

### RP2040-Zero pad diagram

Same GPIO numbers, different physical package — the Zero exposes its pins as
23 numbered solder pads around the board's left, bottom, and right edges
(9 + 5 + 9) rather than a 40-pin DIP like the Pico, and has no
VBUS/VSYS/3V3_EN/ADC_VREF pads at all. Pad numbers follow Waveshare's own
convention (numbered anticlockwise from the USB-C connector, down the left
edge, across the bottom, up the right edge).

```
                  ┌───────────────────┐
                  │   USB-C     (LED) │  ← onboard NeoPixel (WS2812) is
                  │      [BOOT]       │    wired to GPIO16 internally —
           5V   1 │ o               o │ 23  GP0     not a pad on this board
          GND   2 │ o               o │ 22  GP1
          3V3   3 │ o               o │ 21  GP2
         GP29   4 │ o               o │ 20  GP3
         GP28   5 │ o               o │ 19  GP4   ◄  SDA
         GP27   6 │ o               o │ 18  GP5   ◄  SCL
         GP26   7 │ o               o │ 17  GP6   ◄  FAN
 PWM  ►  GP15   8 │ o               o │ 16  GP7
         GP14   9 │ o               o │ 15  GP8
                  └──o───o───o───o───o┘
                     |   |   |   |   |
                     10   1   1   1   1
                     0   1   2   3   4
                     |   |   |   |   |
                     G   G   G   G   G
                     P   P   P   P   P
                     1   1   1   1
                     3   2   1   0   9  ◄  TACH
```

No VSYS/VBUS equivalent is broken out on the main pad ring — the 5V pad
(1) is the closest analog; see "Powering the RP2040 itself" below.
GPIO17–25 exist on this chip but aren't on this ring at all (Waveshare
breaks a few of them out via separate small solder-only headers elsewhere
on the PCB) — this board doesn't use any of them, so they're omitted here.

## Fan (Delta Electronics BFB1012MD)

Not part of the 14-pin harness — this is a standard PC-style 4-pin PWM fan
connector, wired directly to the Pico. Pin assignments come from `board.rs`
(`FanPwmPin`/`FanTachPin`); the control loop itself lives in `fan.rs`.

| Fan connector pin | RP2040 | Pico pin | RP2040-Zero pad | `board.rs`   |
|-------------------|--------|----------|-----------------|--------------|
| 1 — GND           | —      | —        | —               | tap harness pin 1/5/7/8            |
| 2 — +12V          | —      | —        | —               | tap harness pin 4 or 10/11 (PP12V) |
| 3 — Tach          | GPIO9  | 12       | 14              | `FanTachPin` |
| 4 — PWM           | GPIO6  | 9        | 17              | `FanPwmPin`  |

That pin *order* (GND, +12V, Tach, PWM) is fixed by the connector spec and
consistent across manufacturers, but wire *colors* vary — confirm which wire
is actually +12V against the BFB1012MD's datasheet or with a meter before
connecting anything to GPIO9/GPIO6. Swapping +12V and Tach would put 12V
directly on an RP2040 GPIO, which is only 3.3V-tolerant and would not
survive it.

GPIO6 and GPIO9 land on different PWM slices (3 and 4) on purpose — reading
the tach by counting edges reclocks its *entire* slice's counter off that
pin, which would break PWM generation if it shared a slice with the output.
See `board.rs`'s comment above `FanPwmSlice` for the full explanation.

PWM runs at 25kHz (`FAN_PWM_FREQUENCY_HZ`), per the Intel 4-Wire PWM Fan
spec's recommended control frequency range. The tach line reads cleanly off
most 4-pin PC fans' open-collector/open-drain output on its own, but
`board.rs` also enables the RP2040's internal pull-up (`Pull::Up`) on GPIO9
as a backstop.

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
- Verify the fan connector's actual pinout against its datasheet before
  wiring GPIO6/GPIO9 to it — see the fan section above. Confirming pin
  *order*, not trusting wire color, is what matters here.
- On an RP2040-Zero build, confirm each pad against its `GP<n>` silkscreen
  label before soldering — worth a quick sanity check even though the pad
  diagram above is now confirmed against the physical board.
- Confirm the PSU is actually producing SMBus activity before assuming a scan
  found nothing — see `smbus.rs`'s module doc comment: `scan_task` gives the
  bus 2 seconds after boot to settle, but if PS_ON isn't asserted the PSU may
  not be running its SMBus interface at all.
