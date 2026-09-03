//! Host-side companion to the CinemaControl firmware (`../firmware`): set,
//! query, and watch a board's brightness and PSU telemetry over USB HID.

mod commands;

use anyhow::{Context, Result};
use argh::FromArgs;
use board_hid::device;
use hidapi::HidApi;

/// Control and monitor CinemaControl USB HID devices.
#[derive(FromArgs)]
struct Cli {
    /// which connected board to target, 0-indexed by ascending USB serial
    /// number (see `device::discover`)
    #[argh(option, short = 'd', default = "0")]
    device: usize,

    #[argh(subcommand)]
    command: Command,
}

#[derive(FromArgs)]
#[argh(subcommand)]
enum Command {
    List(ListArgs),
    GetBrightness(GetBrightnessArgs),
    SetBrightness(SetBrightnessArgs),
    GetPsu(GetPsuArgs),
    Watch(WatchArgs),
}

/// List every connected board and its index.
#[derive(FromArgs)]
#[argh(subcommand, name = "list")]
struct ListArgs {}

/// Read the current brightness (0..=1023).
#[derive(FromArgs)]
#[argh(subcommand, name = "get-brightness")]
struct GetBrightnessArgs {}

/// Set the brightness (0..=1023, clamped).
#[derive(FromArgs)]
#[argh(subcommand, name = "set-brightness")]
struct SetBrightnessArgs {
    #[argh(positional)]
    value: u16,
}

/// Read the current PSU telemetry.
#[derive(FromArgs)]
#[argh(subcommand, name = "get-psu")]
struct GetPsuArgs {}

/// Stream brightness and PSU updates as they change, until interrupted.
#[derive(FromArgs)]
#[argh(subcommand, name = "watch")]
struct WatchArgs {
    /// print one combined line (the latest of all three values) on every
    /// update, instead of a separate line per interface that changed
    #[argh(switch, short = 'c')]
    combined: bool,
}

fn main() -> Result<()> {
    let cli: Cli = argh::from_env();
    let api = HidApi::new().context("initializing HID backend")?;
    let boards = device::discover(&api).context("enumerating CinemaControl devices")?;

    if let Command::List(_) = cli.command {
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
        Command::List(_) => unreachable!("handled above"),
        Command::GetBrightness(_) => commands::get_brightness(&api, board),
        Command::SetBrightness(SetBrightnessArgs { value }) => {
            commands::set_brightness(&api, board, value)
        }
        Command::GetPsu(_) => commands::get_psu(&api, board),
        Command::Watch(WatchArgs { combined }) => commands::watch(&api, board, combined),
    }
}
