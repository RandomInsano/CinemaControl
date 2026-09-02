//! WS2812 ("NeoPixel") on GPIO16 (`neopixel` feature — see `board.rs`): the
//! RP2040-Zero's onboard one, or an external one wired to that pin on a
//! Pico. Mirrors the current backlight brightness at up to 50% intensity, so
//! it can be enabled as a visible-through-the-shell brightness indicator.

use smart_leds::RGB8;

use crate::board::Neopixel;
use crate::hid;

/// Flip to `false` to disable the mirroring behavior entirely (the LED then
/// just stays off).
pub const ENABLED: bool = true;

/// 50% of `u8::MAX`.
const MAX_INTENSITY: u32 = 127;

#[embassy_executor::task]
pub async fn task(mut ws2812: Neopixel) -> ! {
    if !ENABLED {
        ws2812.write(&[RGB8::default()]).await;
        core::future::pending::<()>().await;
        unreachable!()
    }

    let mut brightness = hid::BRIGHTNESS.receiver().unwrap();
    let mut level = brightness.try_get().unwrap();
    let mut last_color = None;

    loop {
        let color = render(level);
        if Some(color) != last_color {
            ws2812.write(&[color]).await;
            last_color = Some(color);
        }
        level = brightness.changed().await;
    }
}

fn render(level: u16) -> RGB8 {
    let scaled = (u32::from(level) * MAX_INTENSITY / u32::from(protocol::MAX_BRIGHTNESS)) as u8;
    RGB8::new(scaled, scaled, scaled)
}
