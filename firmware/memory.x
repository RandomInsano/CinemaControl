/* Raspberry Pi Pico (RP2040): 2048K QSPI flash / 264K RAM.
 *
 * BOOT2 holds the second-stage bootloader that embassy-rp/cortex-m-rt links
 * into the image (via `-Tlink-rp.x` in `.cargo/config.toml`) and must start
 * at flash offset 0.
 *
 * The last two 4K erase sectors are reserved for brightness persistence
 * storage — see src/storage.rs — by ending FLASH two sectors early.
 * sequential-storage (used there) needs at least two sectors to do its
 * wear-leveling/compaction.
 */
MEMORY
{
  BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
  FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100 - 8K
  RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}
