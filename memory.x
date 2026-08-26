/* STM32F103C8T6 ("Blue Pill"), as documented: 64K flash / 20K RAM.
 * Many clone boards actually carry a 128K-flash die (STM32F103C8T6 clones are
 * commonly re-marked STM32F103CB or genuinely wider parts) — if `probe-rs info`
 * or a full-chip-erase/read shows more flash available, widen FLASH LENGTH to
 * 128K here to use it.
 */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 64K
  RAM   : ORIGIN = 0x20000000, LENGTH = 20K
}
