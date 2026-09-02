//! A per-board submenu (title, brightness/power/temp rows, slider), built
//! entirely from raw AppKit rather than muda's `Submenu`.
//!
//! Muda re-synthesizes a `Submenu`'s displayed `NSMenu` from scratch the
//! moment it's attached to a parent menu — see `create_ns_item_for_submenu`
//! in muda's macOS backend — which silently orphans anything inserted into
//! the `Submenu`'s own `ContextMenu::ns_menu()` beforehand, including our
//! slider. Top-level `Menu`s aren't re-synthesized this way (tray-icon uses
//! their `ns_menu()` directly), so building the whole per-board row by hand
//! and inserting it into the *top-level* menu sidesteps the problem.

use std::ffi::c_void;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::NSString;

use super::slider::BrightnessSlider;

pub struct BoardMenu {
    pub item: Retained<NSMenuItem>,
    brightness_item: Retained<NSMenuItem>,
    power_item: Retained<NSMenuItem>,
    thermal_item: Retained<NSMenuItem>,
    chip_temp_item: Retained<NSMenuItem>,
    pub slider: BrightnessSlider,
}

impl BoardMenu {
    pub fn new(
        serial: &str,
        brightness_text: &str,
        power_text: &str,
        thermal_text: &str,
        chip_temp_text: &str,
        initial_percent: u32,
    ) -> Self {
        let mtm = MainThreadMarker::new().expect("must run on the main thread");

        let submenu = NSMenu::new(mtm);
        submenu.setAutoenablesItems(false);

        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(serial));
        item.setSubmenu(Some(&submenu));

        let brightness_item = label_item(mtm, brightness_text);
        let power_item = label_item(mtm, power_text);
        let thermal_item = label_item(mtm, thermal_text);
        let chip_temp_item = label_item(mtm, chip_temp_text);
        submenu.addItem(&brightness_item);
        submenu.addItem(&power_item);
        submenu.addItem(&thermal_item);
        submenu.addItem(&chip_temp_item);

        // Inserted after `brightness_item` (index 0), ahead of power/thermal.
        let slider = BrightnessSlider::insert(
            Retained::as_ptr(&submenu) as *mut c_void,
            1,
            initial_percent,
        );

        Self {
            item,
            brightness_item,
            power_item,
            thermal_item,
            chip_temp_item,
            slider,
        }
    }

    pub fn set_brightness_text(&self, text: &str) {
        self.brightness_item.setTitle(&NSString::from_str(text));
    }

    pub fn set_power_text(&self, text: &str) {
        self.power_item.setTitle(&NSString::from_str(text));
    }

    pub fn set_thermal_text(&self, text: &str) {
        self.thermal_item.setTitle(&NSString::from_str(text));
    }

    pub fn set_chip_temp_text(&self, text: &str) {
        self.chip_temp_item.setTitle(&NSString::from_str(text));
    }
}

fn label_item(mtm: MainThreadMarker, text: &str) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(text));
    item.setEnabled(false);
    item
}
