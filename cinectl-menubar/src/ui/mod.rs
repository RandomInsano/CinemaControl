//! AppKit menu bar UI: the tray icon and each board's submenu, slider
//! included. Separated from `device.rs`/`report.rs` (the HID transport
//! logic) and `main.rs` (orchestration).

pub mod board_menu;
pub mod icon;
pub mod slider;
