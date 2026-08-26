/* STM32F103C8T6 ("Blue Pill"), as documented: 64K flash / 20K RAM.
 * Many clone boards actually carry a 128K-flash die (STM32F103C8T6 clones are
 * commonly re-marked STM32F103CB or genuinely wider parts) — if `probe-rs info`
 * or a full-chip-erase/read shows more flash available, widen FLASH LENGTH to
 * 128K here to use it (and move the reserved page below to stay at the end).
 *
 * The last two 1K pages (0x0800F800-0x0800FFFF) are reserved for brightness
 * persistence storage — see src/storage.rs — by ending FLASH two pages
 * early. sequential-storage (used there) needs at least two pages to do its
 * wear-leveling/compaction.
 */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 62K
  RAM   : ORIGIN = 0x20000000, LENGTH = 20K
}
