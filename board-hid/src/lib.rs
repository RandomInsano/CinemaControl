//! Host-side HID transport for CinemaControl boards: discovering connected
//! boards and reading/writing their feature reports. Shared by `cinectl`
//! and `cinectl-menubar` — see the repo's `AGENTS.md`.

pub mod device;
pub mod report;
pub mod telemetry;
pub mod transport;

pub use device::{Board, discover};
