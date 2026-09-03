//! Minimal macOS menu bar companion for CinemaControl boards: shows the PSU
//! power/temperature readings and lets you drag brightness from the menu
//! bar, without needing a terminal open. Each connected board gets its own
//! submenu; boards can come and go (hot-plug/unplug, or none at all at
//! launch) without a restart.
//!
//! Nothing reads a board until the user actually opens the menu, at which
//! point every currently-plugged-in board is queried in parallel (bounded
//! by `REFRESH_TIMEOUT` so one stalled device can't hold up the rest) and
//! the top-level menu is rebuilt to match. From then on, for as long as
//! the board stays connected, its text is kept live by four background
//! threads (one per interface) blocking-reading the device's pushed input
//! reports — see `board_hid::transport::stream_input` — rather than by
//! polling: a report only arrives when the value actually changes, so an
//! idle board generates no traffic between updates. Text only, though:
//! the brightness slider is write-only from the app's side, moved only by
//! the user's own drag and never reassigned from a telemetry push — doing
//! that was the root cause of several past rounds of slider jumpiness.
//!
//! Board discovery, HID transport, and the report wire format live in
//! `board-hid`, shared with `cinectl`.

mod login_item;
mod ui;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use block2::RcBlock;
use board_hid::device::{self, Board};
use board_hid::telemetry::{
    read_brightness, read_power, read_power_thermal, read_processor_thermal, stream_brightness,
    stream_power, stream_power_thermal, stream_processor_thermal,
};
use board_hid::transport::{open, require_path};
use hidapi::HidApi;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSMenu};
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};
use protocol::{MAX_BRIGHTNESS, PowerTelemetry, PowerThermalTelemetry, ProcessorThermalTelemetry};
use tray_icon::menu::{CheckMenuItem, ContextMenu, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIconBuilder, TrayIconEvent};

use ui::board_menu::BoardMenu;
use ui::icon;
use ui::menu_delegate::MenuDelegate;

const POLL_INTERVAL: Duration = Duration::from_millis(250);
// How long a menu-open refresh waits on any one board before giving up on
// it for this round and showing whatever it last had.
const REFRESH_TIMEOUT: Duration = Duration::from_millis(200);
const DISCONNECTED_TEXT: &str = "No CinemaControl device found";

fn main() -> Result<()> {
    let api = HidApi::new().context("initializing HID backend")?;

    let placeholder_item = MenuItem::new(DISCONNECTED_TEXT, false, None);
    let login_item_item = CheckMenuItem::with_id(
        "start-at-login",
        "Start at Login",
        true,
        login_item::is_enabled(),
        None,
    );
    let quit_item = MenuItem::with_id("quit", "Quit", true, None);

    let menu = Menu::new();
    menu.append_items(&[
        &placeholder_item,
        &PredefinedMenuItem::separator(),
        &login_item_item,
        &quit_item,
    ])
    .context("building tray menu")?;
    // Board rows are inserted/removed as raw `NSMenuItem`s (see
    // `board_menu.rs`), so we manipulate the top-level menu's NSMenu
    // directly for those rather than going through muda's `Menu::insert`.
    let top_ns_menu = menu.ns_menu();

    // Held for the rest of `main` so the status item stays alive; nothing
    // needs to touch it again now that the icon is static.
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu.clone()))
        .with_tooltip("CinemaControl")
        .with_icon(icon::render_sf_symbol("lightbulb.fill", 18.0)?)
        .with_icon_as_template(true)
        .build()
        .context("creating tray icon")?;

    let mtm = MainThreadMarker::new().expect("must run on the main thread");
    let state = Rc::new(RefCell::new(AppState {
        api,
        boards: BTreeMap::new(),
        menu: menu.clone(),
        placeholder_item: placeholder_item.clone(),
        placeholder_shown: true,
        refreshed_this_open: false,
        top_ns_menu,
    }));
    // Refreshes board data synchronously, right before the menu is shown —
    // see menu_delegate.rs for why this can't just happen in response to
    // the tray icon's click event instead. Held for the rest of `main` so
    // the (weak) delegate reference on the menu stays valid.
    let _menu_delegate = MenuDelegate::new(mtm, Rc::clone(&state));
    top_menu(top_ns_menu).setDelegate(Some(MenuDelegate::as_protocol_object(&_menu_delegate)));

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let _timer = schedule_tick(state, login_item_item);

    app.run();
    Ok(())
}

/// Drives [`tick`] every [`POLL_INTERVAL`] for as long as the returned
/// `NSTimer` (held by the caller) stays alive.
///
/// Added to the main run loop's *common* modes by hand, rather than via
/// `NSTimer`'s usual auto-scheduling (which only covers
/// `NSDefaultRunLoopMode`): AppKit runs menu tracking in its own
/// `NSEventTrackingRunLoopMode`, so without common modes this tick — and
/// with it drag polling — would silently stop firing the moment a submenu
/// is actually open, exactly when it matters most.
fn schedule_tick(
    state: Rc<RefCell<AppState>>,
    login_item_item: CheckMenuItem,
) -> Retained<NSTimer> {
    let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
        tick(&state, &login_item_item);
    });
    unsafe {
        let timer =
            NSTimer::timerWithTimeInterval_repeats_block(POLL_INTERVAL.as_secs_f64(), true, &block);
        NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
        timer
    }
}

fn tick(state: &Rc<RefCell<AppState>>, login_item_item: &CheckMenuItem) {
    // The discovery refresh happens in MenuDelegate::menuNeedsUpdate,
    // synchronously before the menu is shown — just drain the channel
    // here so it doesn't grow unbounded.
    while TrayIconEvent::receiver().try_recv().is_ok() {}

    // `try_borrow_mut`, not `borrow_mut`: this can run re-entrantly while
    // `MenuDelegate::menuNeedsUpdate` (which holds its own borrow for the
    // ~200ms it may spend on I/O) is still on the stack. Skipping a tick
    // here is harmless — telemetry draining and the write-failed check
    // just pick back up 250ms later — panicking is not.
    if let Ok(mut state) = state.try_borrow_mut() {
        let mut disconnected = Vec::new();
        for (serial, board) in state.boards.iter_mut() {
            // Applies whatever telemetry pushes have arrived since the
            // last tick — cheap even when nothing has, since it's a
            // non-blocking drain of however many background reader
            // threads (see `TelemetryStreams`) have sent so far.
            board.streams.drain_into(&board.menu);
            // The slider itself writes brightness straight from its own
            // target/action callback (see `ui::slider`), not on this
            // timer — this only checks whether that last write failed, the
            // same signal `poll_drag` used to use to catch an unplugged
            // board immediately rather than waiting for the next
            // menu-open discovery.
            if board.menu.slider.write_failed() {
                disconnected.push(serial.clone());
            }
        }
        // Drop tracking (so we stop draining/checking it) but don't touch
        // its still-visible NSMenuItem here — this tick runs on a timer
        // regardless of whether the menu is currently open, and removing
        // a top-level item from an open menu is the same bug
        // menuNeedsUpdate exists to avoid, just via a different trigger.
        // Its stale submenu just sits there until the next open's
        // discovery (safely, pre-display) confirms it's really gone and
        // cleans it up then.
        for serial in disconnected {
            state.boards.remove(&serial);
            eprintln!("CinemaControl device {serial:?} disconnected");
        }
    }

    while let Ok(event) = MenuEvent::receiver().try_recv() {
        match event.id().0.as_str() {
            "quit" => {
                let mtm = MainThreadMarker::new().expect("must run on the main thread");
                NSApplication::sharedApplication(mtm).terminate(None);
            }
            "start-at-login" => {
                let enable = !login_item_item.is_checked();
                match login_item::set_enabled(enable) {
                    Ok(()) => login_item_item.set_checked(enable),
                    Err(e) => eprintln!("failed to update login item: {e:#}"),
                }
            }
            _ => {}
        }
    }
}

fn top_menu(ns_menu: *mut c_void) -> &'static NSMenu {
    unsafe { &*ns_menu.cast::<NSMenu>() }
}

/// All state a menu refresh touches, shared between `tick` (drag polling,
/// menu/tray events) and `MenuDelegate` (populating the menu right before
/// it's shown).
pub(crate) struct AppState {
    api: HidApi,
    boards: BTreeMap<String, BoardState>,
    menu: Menu,
    placeholder_item: MenuItem,
    placeholder_shown: bool,
    top_ns_menu: *mut c_void,
    /// Set by `MenuDelegate::menuNeedsUpdate` once it's refreshed for the
    /// menu's current open session, cleared by `menuDidClose` — caps it at
    /// one refresh per open even if AppKit calls `menuNeedsUpdate` more
    /// than once while it's up.
    pub(crate) refreshed_this_open: bool,
}

impl AppState {
    /// Re-discovers every connected board and queries each in parallel
    /// (each bounded by `REFRESH_TIMEOUT`), updating `boards` and the menu
    /// in place: new boards get a submenu, existing ones get fresh text,
    /// and boards no longer found at all are torn down. Safe to call while
    /// the menu is *not yet* visible (i.e. from `menuNeedsUpdate:`) —
    /// inserting/removing top-level items while it's actually on screen is
    /// what causes AppKit to dismiss it.
    fn refresh(&mut self) {
        refresh(&mut self.api, &mut self.boards, self.top_ns_menu);

        if self.boards.is_empty() && !self.placeholder_shown {
            self.menu
                .insert(&self.placeholder_item, 0)
                .expect("inserting placeholder");
            self.placeholder_shown = true;
        } else if !self.boards.is_empty() && self.placeholder_shown {
            self.menu
                .remove(&self.placeholder_item)
                .expect("removing placeholder");
            self.placeholder_shown = false;
        }
    }
}

/// Re-discovers every connected board and queries each in parallel (each
/// bounded by `REFRESH_TIMEOUT`), updating `boards` and the menu in place:
/// new boards get a submenu, existing ones get fresh text, and boards no
/// longer found at all are torn down.
fn refresh(api: &mut HidApi, boards: &mut BTreeMap<String, BoardState>, top_ns_menu: *mut c_void) {
    if let Err(e) = api.refresh_devices() {
        eprintln!("failed to refresh HID device list: {e}");
        return;
    }
    let discovered = match device::discover(api) {
        Ok(boards) => boards,
        Err(e) => {
            eprintln!("failed to enumerate CinemaControl devices: {e}");
            return;
        }
    };
    let discovered_serials: BTreeSet<String> =
        discovered.iter().map(|b| b.serial.clone()).collect();

    // Only boards `apply` hasn't seen yet need a read here — an
    // already-tracked board's text is being kept current continuously by
    // its own `TelemetryStreams`, so re-reading it on every open would
    // just be redundant HID traffic.
    let new_boards = discovered
        .into_iter()
        .filter(|board| !boards.contains_key(&board.serial));

    let (tx, rx) = mpsc::channel();
    for board in new_boards {
        let tx = tx.clone();
        thread::spawn(move || {
            let result = HidApi::new()
                .context("initializing HID backend")
                .and_then(|api| read_telemetry(&api, &board));
            let _ = tx.send((board, result));
        });
    }
    drop(tx);

    let deadline = Instant::now() + REFRESH_TIMEOUT;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(remaining) {
            Ok((board, Ok(telemetry))) => apply(api, board, telemetry, boards, top_ns_menu),
            Ok((board, Err(e))) => {
                eprintln!(
                    "failed to read CinemaControl device {:?}: {e}",
                    board.serial
                )
            }
            Err(_) => break,
        }
    }

    let gone: Vec<String> = boards
        .keys()
        .filter(|serial| !discovered_serials.contains(*serial))
        .cloned()
        .collect();
    for serial in gone {
        if let Some(state) = boards.remove(&serial) {
            top_menu(top_ns_menu).removeItem(&state.menu.item);
        }
    }
}

/// Builds a new board's menu and adds it: opens a write handle it keeps
/// for slider drags, and starts its `TelemetryStreams` for as long as it
/// stays connected. Only ever called (by `refresh`) for a board that
/// isn't already tracked.
fn apply(
    api: &HidApi,
    board: Board,
    telemetry: Telemetry,
    boards: &mut BTreeMap<String, BoardState>,
    top_ns_menu: *mut c_void,
) {
    let write_device =
        match require_path(&board.brightness_path, "brightness").and_then(|path| open(api, path)) {
            Ok(device) => device,
            Err(e) => {
                eprintln!(
                    "failed to open CinemaControl device {:?} for writing: {e}",
                    board.serial
                );
                return;
            }
        };

    let menu = BoardMenu::new(
        &board.serial,
        &brightness_text(telemetry.brightness),
        &format!("Power: {}", telemetry.power),
        &format!("Temp: {}", telemetry.power_thermal),
        &format!("MCU: {}", telemetry.processor_thermal),
        telemetry.brightness,
        write_device,
    );
    let position = boards.len() as isize;
    top_menu(top_ns_menu).insertItem_atIndex(&menu.item, position);

    let streams = TelemetryStreams::open(api, &board);
    boards.insert(board.serial, BoardState { menu, streams });
}

/// A board's four telemetry interfaces, each streamed by its own
/// background thread (`board_hid::transport::stream_input`) for as long
/// as `apply` was able to open it — `None` for an interface a board's
/// firmware predates, same as the one-shot reads' `unwrap_or_default`.
struct TelemetryStreams {
    brightness: Option<Receiver<Result<u16>>>,
    power: Option<Receiver<Result<PowerTelemetry>>>,
    power_thermal: Option<Receiver<Result<PowerThermalTelemetry>>>,
    processor_thermal: Option<Receiver<Result<ProcessorThermalTelemetry>>>,
}

impl TelemetryStreams {
    fn open(api: &HidApi, board: &Board) -> Self {
        Self {
            brightness: stream_brightness(api, board).ok(),
            power: stream_power(api, board).ok(),
            power_thermal: stream_power_thermal(api, board).ok(),
            processor_thermal: stream_processor_thermal(api, board).ok(),
        }
    }

    /// Applies every update that's arrived on any of the four streams
    /// since the last call, updating `menu`'s text in place. Never
    /// touches the slider — see the module doc for why.
    fn drain_into(&mut self, menu: &BoardMenu) {
        drain(&mut self.brightness, |v| {
            menu.set_brightness_text(&brightness_text(v))
        });
        drain(&mut self.power, |v| {
            menu.set_power_text(&format!("Power: {v}"))
        });
        drain(&mut self.power_thermal, |v| {
            menu.set_power_thermal_text(&format!("Temp: {v}"))
        });
        drain(&mut self.processor_thermal, |v| {
            menu.set_processor_thermal_text(&format!("MCU: {v}"))
        });
    }
}

/// Applies every value already sitting in `stream`, in order, without
/// blocking. Clears `stream` to `None` (so future calls are a no-op) once
/// its reader thread has ended — a read error or the device going away —
/// since nothing more will ever arrive on it.
fn drain<T>(stream: &mut Option<Receiver<Result<T>>>, mut apply: impl FnMut(T)) {
    let Some(rx) = stream.as_ref() else {
        return;
    };
    loop {
        match rx.try_recv() {
            Ok(Ok(value)) => apply(value),
            Ok(Err(e)) => {
                eprintln!("telemetry stream ended: {e:#}");
                *stream = None;
                return;
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                *stream = None;
                return;
            }
        }
    }
}

struct BoardState {
    menu: BoardMenu,
    streams: TelemetryStreams,
}

struct Telemetry {
    brightness: u16,
    power: PowerTelemetry,
    power_thermal: PowerThermalTelemetry,
    processor_thermal: ProcessorThermalTelemetry,
}

/// Only `brightness` is required for a board to be shown at all — it's the
/// one interface every CinemaControl firmware has ever shipped with, so a
/// board that fails to answer it isn't meaningfully "there." The PSU/thermal
/// interfaces are best-effort: a board whose firmware predates one of them
/// (e.g. `processor_thermal`) still shows up, just with that field defaulted.
fn read_telemetry(api: &HidApi, board: &Board) -> Result<Telemetry> {
    Ok(Telemetry {
        brightness: read_brightness(api, board)?,
        power: read_power(api, board).unwrap_or_default(),
        power_thermal: read_power_thermal(api, board).unwrap_or_default(),
        processor_thermal: read_processor_thermal(api, board).unwrap_or_default(),
    })
}

fn brightness_text(value: u16) -> String {
    format!("Brightness: {}%", percent(value))
}

fn percent(value: u16) -> u32 {
    (u32::from(value) * 100 + u32::from(MAX_BRIGHTNESS) / 2) / u32::from(MAX_BRIGHTNESS)
}
