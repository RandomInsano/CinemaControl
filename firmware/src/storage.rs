//! Persists brightness across power cycles in the last two flash erase
//! sectors (reserved for this in `memory.x`), debounced so dragging a
//! brightness slider doesn't wear out the flash.
//!
//! Storage itself — wear-leveling, power-fail safety, CRC-checked items —
//! is handled by the `sequential-storage` crate rather than hand-rolled
//! here. Unlike the STM32F1 (Blue Pill) case, `embassy-rp`'s async `Flash`
//! implements `embedded-storage-async` directly, so no blocking-to-async
//! adapter is needed.

use core::ops::Range;

use defmt::warn;
use sequential_storage::cache::{Cache, Uncached};
use sequential_storage::map::{MapConfig, MapStorage};

use crate::board::{BoardFlash, FLASH_SIZE};
use crate::hid;

/// Last two 4 KiB erase sectors; `memory.x` reserves them by ending the
/// linker's `FLASH` region 8K early.
const FLASH_RANGE: Range<u32> = (FLASH_SIZE as u32 - 8 * 1024)..FLASH_SIZE as u32;

const DEBOUNCE: embassy_time::Duration = embassy_time::Duration::from_secs(30);

type Store = MapStorage<(), BoardFlash, Cache<Uncached, Uncached, Uncached, ()>>;

pub fn init(flash: BoardFlash) -> Store {
    MapStorage::new(
        flash,
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
    let mut brightness = hid::BRIGHTNESS.receiver().unwrap();
    loop {
        let saved = brightness.changed().await;
        save(&mut store, saved).await;

        embassy_time::Timer::after(DEBOUNCE).await;
        if let Some(v) = brightness.try_changed()
            && v != saved
        {
            save(&mut store, v).await;
        }
    }
}

/// Writes `value` to flash, unless it already matches what's stored — so
/// repeated writes of an unchanged brightness don't wear the flash.
async fn save(store: &mut Store, value: u16) {
    if load(store).await == Some(value) {
        return;
    }

    let mut buf = [0u8; 32];
    if let Err(e) = store.store_item(&mut buf, &(), &value).await {
        warn!("brightness flash write failed: {:?}", e);
    }
}
