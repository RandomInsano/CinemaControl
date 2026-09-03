//! A native `NSSlider` embedded as a menu item's view.
//!
//! `tray-icon`/`muda` are cross-platform, so their menu items are limited
//! to text, checkboxes, and submenus — no slider. Muda deliberately exposes
//! the raw `NSMenu*` via `ContextMenu::ns_menu()` for exactly this kind of
//! platform-specific extension, so this builds a real `NSSlider` and drops
//! it into the menu the way Apple's own menu bar extras (volume,
//! brightness) do it.
//!
//! Wired with a real target/action (`setContinuous(true)` so it fires on
//! every drag tick, not just mouse-up) rather than polled: `SliderTarget`
//! owns the board's write handle and writes+echoes a new brightness
//! straight from the callback, so there's nothing to check on a timer.
//!
//! It's write-only from the app's side: nothing ever calls
//! `setDoubleValue` again after `insert` sets the initial position. A
//! telemetry read that disagreed with wherever the user last left the
//! knob would otherwise yank it out from under a drag in progress — the
//! cause of several past rounds of jumpiness — so the knob only moves in
//! response to the user's own input, and reported brightness is shown as
//! text instead (see `brightness_text` in `main.rs`).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;

use hidapi::HidDevice;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSMenu, NSMenuItem, NSSlider, NSView};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use board_hid::report;

const WIDTH: f64 = 170.0;
// Taller than the slider itself needs, so its row gets enough breathing
// room that the knob doesn't crowd the item above it.
const HEIGHT: f64 = 28.0;
const INSET: f64 = 18.0;

struct SliderTargetIvars {
    write_device: HidDevice,
    brightness_item: Retained<NSMenuItem>,
    write_failed: Rc<Cell<bool>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CinectlBrightnessSliderTarget"]
    #[ivars = RefCell<SliderTargetIvars>]
    struct SliderTarget;

    unsafe impl NSObjectProtocol for SliderTarget {}

    impl SliderTarget {
        #[unsafe(method(sliderChanged:))]
        fn slider_changed(&self, sender: &NSSlider) {
            let percent = sender.doubleValue().round() as u32;
            let brightness = crate::brightness_from_percent(percent);
            let ivars = self.ivars().borrow();
            let report = report::brightness_feature_report(brightness);
            if ivars.write_device.send_feature_report(&report).is_ok() {
                ivars
                    .brightness_item
                    .setTitle(&NSString::from_str(&crate::brightness_text(brightness)));
            } else {
                ivars.write_failed.set(true);
            }
        }
    }
);

impl SliderTarget {
    fn new(
        mtm: MainThreadMarker,
        write_device: HidDevice,
        brightness_item: Retained<NSMenuItem>,
        write_failed: Rc<Cell<bool>>,
    ) -> Retained<Self> {
        let ivars = RefCell::new(SliderTargetIvars {
            write_device,
            brightness_item,
            write_failed,
        });
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }
}

pub struct BrightnessSlider {
    // Kept alive for as long as the slider itself: `NSControl::setTarget`
    // holds it unretained (a plain `assign`/weak reference, per AppKit
    // convention), so nothing else keeps this object alive otherwise. The
    // `NSSlider` itself needs no field here — the container view (added to
    // the menu item) already retains it.
    _target: Retained<SliderTarget>,
    write_failed: Rc<Cell<bool>>,
}

impl BrightnessSlider {
    /// Builds the slider and inserts it as a menu item at `index` into
    /// `ns_menu`, which must be a valid `NSMenu*` (as returned by muda's
    /// `ContextMenu::ns_menu()`) that outlives this call. `write_device` is
    /// the board's already-open brightness HID handle, moved in so the
    /// slider can write straight from its own target/action callback.
    pub fn insert(
        ns_menu: *mut c_void,
        index: isize,
        initial_percent: u32,
        write_device: HidDevice,
        brightness_item: Retained<NSMenuItem>,
    ) -> Self {
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
        control.setContinuous(true);
        container.addSubview(&control);

        let write_failed = Rc::new(Cell::new(false));
        let target = SliderTarget::new(mtm, write_device, brightness_item, Rc::clone(&write_failed));
        unsafe {
            control.setTarget(Some(&target));
            control.setAction(Some(sel!(sliderChanged:)));
        }

        let item = NSMenuItem::new(mtm);
        item.setView(Some(&container));

        let menu: &NSMenu = unsafe { &*ns_menu.cast::<NSMenu>() };
        menu.insertItem_atIndex(&item, index);

        Self {
            _target: target,
            write_failed,
        }
    }

    /// Whether the slider's last write failed — a board dropping its
    /// brightness interface mid-drag, almost always because it was
    /// unplugged. Checked once per tick so a dead board doesn't have to
    /// wait for the next menu-open discovery to be torn down.
    pub fn write_failed(&self) -> bool {
        self.write_failed.get()
    }
}
