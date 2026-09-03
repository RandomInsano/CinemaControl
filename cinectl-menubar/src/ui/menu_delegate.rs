//! An `NSMenuDelegate` that refreshes board data — and activates the app —
//! synchronously, right before the menu is shown.
//!
//! Refresh: doing this from a `TrayIconEvent::Click` handler (processed on
//! the next timer tick after the click — see `main.rs`'s `tick`) was too
//! late — `performClick` shows
//! the menu synchronously on `mouseDown`, so by the time our refresh ran
//! the menu was already open, and inserting/removing a top-level item into
//! an *already-visible* menu makes AppKit dismiss it. `menuNeedsUpdate:`
//! runs before AppKit displays the popup, so mutating the menu there is
//! safe. It isn't guaranteed to fire only once per open (AppKit may call
//! it repeatedly for menus with live content), so `refreshed_this_open`
//! caps it at one refresh per session; `menuDidClose:` resets that flag.
//!
//! Activation: confirmed via `runningboardd`/`launchservicesd` logs that a
//! fully-populated menu closing immediately coincides with *our own* app
//! being granted a "frontmost" assertion (demoting whatever had focus)
//! right as the menu opens — an activation transition AppKit sometimes
//! performs implicitly as part of showing a status item's menu, racing
//! with the menu's own tracking-loop setup. A popup aborting when its
//! owning app's activation state changes mid-track is normal AppKit
//! behavior; the bug was that transition landing *after* tracking had
//! already started. `menuWillOpen:` does it ourselves, synchronously, up
//! front, so there's nothing left to transition once tracking begins.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuDelegate};
use objc2_foundation::{NSObject, NSObjectProtocol};

use crate::AppState;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CinectlMenuDelegate"]
    #[ivars = Rc<RefCell<AppState>>]
    pub(crate) struct MenuDelegate;

    unsafe impl NSObjectProtocol for MenuDelegate {}

    unsafe impl NSMenuDelegate for MenuDelegate {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, _menu: &NSMenu) {
            let mut state = self.ivars().borrow_mut();
            if !state.refreshed_this_open {
                state.refreshed_this_open = true;
                state.refresh();
            }
        }

        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, _menu: &NSMenu) {
            let mtm = MainThreadMarker::new().expect("must run on the main thread");
            // `activate` (the non-deprecated replacement) explicitly
            // doesn't guarantee synchronous, immediate activation — the
            // opposite of what's needed here to close the race window.
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }

        #[unsafe(method(menuDidClose:))]
        fn menu_did_close(&self, _menu: &NSMenu) {
            self.ivars().borrow_mut().refreshed_this_open = false;
        }
    }
);

impl MenuDelegate {
    pub(crate) fn new(mtm: MainThreadMarker, state: Rc<RefCell<AppState>>) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(state);
        unsafe { msg_send![super(this), init] }
    }

    pub(crate) fn as_protocol_object(this: &Retained<Self>) -> &ProtocolObject<dyn NSMenuDelegate> {
        ProtocolObject::from_ref(&**this)
    }
}
