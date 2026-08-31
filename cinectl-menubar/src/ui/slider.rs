//! A native `NSSlider` embedded as a menu item's view.
//!
//! `tray-icon`/`muda` are cross-platform, so their menu items are limited
//! to text, checkboxes, and submenus — no slider. Muda deliberately exposes
//! the raw `NSMenu*` via `ContextMenu::ns_menu()` for exactly this kind of
//! platform-specific extension, so this builds a real `NSSlider` and drops
//! it into the menu the way Apple's own menu bar extras (volume,
//! brightness) do it. The slider isn't wired to a target/action; `main.rs`
//! just polls `percent()` on the same timer it uses for everything else.

use std::ffi::c_void;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSMenu, NSMenuItem, NSSlider, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};

const WIDTH: f64 = 170.0;
// Taller than the slider itself needs, so its row gets enough breathing
// room that the knob doesn't crowd the item above it.
const HEIGHT: f64 = 28.0;
const INSET: f64 = 18.0;

pub struct BrightnessSlider {
    control: Retained<NSSlider>,
}

impl BrightnessSlider {
    /// Builds the slider and inserts it as a menu item at `index` into
    /// `ns_menu`, which must be a valid `NSMenu*` (as returned by muda's
    /// `ContextMenu::ns_menu()`) that outlives this call.
    pub fn insert(ns_menu: *mut c_void, index: isize, initial_percent: u32) -> Self {
        let mtm = MainThreadMarker::new().expect("must run on the main thread");

        // AppKit resets a menu item's *own* custom view to origin (0, 0)
        // when laying out the menu — only its size is honored — so an
        // inset on the slider's own frame is silently ignored. Giving the
        // item a plain container view instead and positioning the slider
        // as an ordinary subview within it works: that's normal view
        // hierarchy layout, not something the menu's own placement touches.
        let container = NSView::initWithFrame(
            mtm.alloc(),
            NSRect::new(NSPoint::ZERO, NSSize::new(INSET + WIDTH, HEIGHT)),
        );

        let slider_frame = NSRect::new(NSPoint::new(INSET, 0.0), NSSize::new(WIDTH, HEIGHT));
        let control = NSSlider::initWithFrame(mtm.alloc(), slider_frame);
        control.setMinValue(0.0);
        control.setMaxValue(100.0);
        control.setDoubleValue(f64::from(initial_percent));
        container.addSubview(&control);

        let item = NSMenuItem::new(mtm);
        item.setView(Some(&container));

        let menu: &NSMenu = unsafe { &*ns_menu.cast::<NSMenu>() };
        menu.insertItem_atIndex(&item, index);

        Self { control }
    }

    pub fn percent(&self) -> u32 {
        self.control.doubleValue().round() as u32
    }

    pub fn set_percent(&self, value: u32) {
        self.control.setDoubleValue(f64::from(value));
    }
}
