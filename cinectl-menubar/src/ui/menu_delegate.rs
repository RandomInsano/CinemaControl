//! An `NSMenuDelegate` that refreshes board data synchronously, right
//! before the menu is shown.
//!
//! Doing this from a `TrayIconEvent::Click` handler (processed on the next
//! tao tick after the click, since that's the only point our code gets to
//! run) was too late: `performClick` already shows the menu synchronously
//! on `mouseDown`, so by the time our refresh ran the menu was already
//! open — and inserting a newly-discovered board's submenu (or removing
//! the placeholder) into an *already-visible* menu made AppKit dismiss it.
//! That showed up as the very first click (the only one where boards goes
//! from empty to populated, so the only one that inserts/removes a
//! top-level item) flashing an empty menu and closing it immediately.
//! `menuNeedsUpdate:` runs before AppKit displays the popup, so mutating
//! the menu there is safe.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSMenu, NSMenuDelegate};
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
            self.ivars().borrow_mut().refresh();
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
