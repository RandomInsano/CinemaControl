//! Registers/unregisters this app as a login item via `SMAppService`
//! (`ServiceManagement.framework`) — the modern replacement for a manual
//! LaunchAgent plist. `SMAppService::mainAppService()` ties directly to
//! the running process's own app bundle, which is why this only works
//! (and only makes sense to call) once the binary runs from inside a
//! properly signed .app bundle — see `packaging/bundle.sh`. Run as a bare
//! binary, `register`/`status` will just report failure/not-found.
//!
//! Confirmed (2026-08-31) that ad-hoc signing isn't enough in practice:
//! `register` fails with "Operation not permitted" — no system consent
//! dialog, no entry in `sfltool dumpbtm` — because Background Task
//! Management can't create an approval record without a stable Team
//! Identifier. `codesign -dv` on an ad-hoc-signed bundle shows
//! `TeamIdentifier=not set`, which is the literal difference. This should
//! start working once the bundle is signed with a real Developer ID
//! (requires a paid Apple Developer Program membership) instead of
//! `packaging/bundle.sh`'s `codesign --sign -`.

use anyhow::{Context, Result};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

pub fn is_enabled() -> bool {
    let service = unsafe { SMAppService::mainAppService() };
    let status = unsafe { service.status() };
    status == SMAppServiceStatus::Enabled
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let service = unsafe { SMAppService::mainAppService() };
    let result = if enabled {
        unsafe { service.registerAndReturnError() }
    } else {
        unsafe { service.unregisterAndReturnError() }
    };
    result
        .map_err(|e| anyhow::anyhow!(e.localizedDescription().to_string()))
        .context("updating login item registration")
}
