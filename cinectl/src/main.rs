//! Host-side companion to the CinemaControl firmware (`../firmware`): set,
//! query, and watch a board's brightness and PSU telemetry over USB HID.

mod commands;
mod device;
mod report;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hidapi::HidApi;

/// Control and monitor CinemaControl USB HID devices.
#[derive(Parser)]
struct Cli {
    /// Which connected board to target, 0-indexed by ascending USB serial
    /// number (see `device::discover`).
    #[arg(short, long, global = true, default_value_t = 0)]
    device: usize,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List every connected board and its index.
    List,
    /// Read the current brightness (0..=1023).
    GetBrightness,
    /// Set the brightness (0..=1023, clamped).
    SetBrightness { value: u16 },
    /// Read the current PSU telemetry.
    GetPsu,
    /// Stream brightness and PSU updates as they change, until interrupted.
    Watch {
        /// Print one combined line (the latest of all three values) on every
        /// update, instead of a separate line per interface that changed.
        #[arg(short, long)]
        combined: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let api = HidApi::new().context("initializing HID backend")?;
    let boards = device::discover(&api).context("enumerating CinemaControl devices")?;

    if let Command::List = cli.command {
        return commands::list(&boards);
    }

    let board = boards.get(cli.device).with_context(|| {
        format!(
            "no CinemaControl device at index {} ({} connected)",
            cli.device,
            boards.len()
        )
    })?;

    match cli.command {
        Command::List => unreachable!("handled above"),
        Command::GetBrightness => commands::get_brightness(&api, board),
        Command::SetBrightness { value } => commands::set_brightness(&api, board, value),
        Command::GetPsu => commands::get_psu(&api, board),
        Command::Watch { combined } => commands::watch(&api, board, combined),
    }
}
