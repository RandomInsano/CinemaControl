//! Persists brightness across power cycles in the last two flash pages
//! (reserved for this in `memory.x`), debounced so dragging a brightness
//! slider doesn't wear out the flash.
//!
//! Storage itself — wear-leveling, power-fail safety, CRC-checked items —
//! is handled by the `sequential-storage` crate rather than hand-rolled
//! here. It only speaks `embedded-storage-async`, and embassy-stm32 has no
//! async flash driver for this chip, so the blocking `Flash` is wrapped in
//! `embassy_embedded_hal`'s `BlockingAsync` adapter.

use core::ops::Range;

use defmt::warn;
use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_stm32::flash::{Blocking, Flash};
use embassy_stm32::peripherals;
use embassy_stm32::Peri;
use embassy_time::{Duration, Timer};
use sequential_storage::cache::{Cache, Uncached};
use sequential_storage::map::{MapConfig, MapStorage};

use crate::hid;

/// Last two 1 KiB pages of the 64 KiB part; `memory.x` reserves them by
/// ending the linker's `FLASH` region at 62K.
const FLASH_RANGE: Range<u32> = 62 * 1024..64 * 1024;

const DEBOUNCE: Duration = Duration::from_secs(30);

type Store = MapStorage<(), BlockingAsync<Flash<'static, Blocking>>, Cache<Uncached, Uncached, Uncached, ()>>;

pub fn init(flash: Peri<'static, peripherals::FLASH>) -> Store {
    MapStorage::new(
        BlockingAsync::new(Flash::new_blocking(flash)),
        const { MapConfig::new(FLASH_RANGE) },
        Cache::new_uncached(),
    )
}

/// Reads the most recently saved brightness, if any.
pub async fn load(store: &mut Store) -> Option<u16> {
    let mut buf = [0u8; 32];
    match store.fetch_item::<u16>(&mut buf, &()).await {
        Ok(value) => value,
        Err(e) => {
            warn!("brightness flash read failed: {:?}", e);
            None
        }
    }
}

#[embassy_executor::task]
pub async fn task(mut store: Store) -> ! {
    loop {
        // Leading edge: write as soon as the value changes.
        let saved = hid::BRIGHTNESS_CHANGED.wait().await;
        save(&mut store, saved).await;

        // Debounce window: absorb further changes, then write once more at
        // the end if the value moved again during it.
        Timer::after(DEBOUNCE).await;
        if let Some(v) = hid::BRIGHTNESS_CHANGED.try_take() {
            if v != saved {
                save(&mut store, v).await;
            }
        }
    }
}

async fn save(store: &mut Store, value: u16) {
    let mut buf = [0u8; 32];
    if let Err(e) = store.store_item(&mut buf, &(), &value).await {
        warn!("brightness flash write failed: {:?}", e);
    }
}
