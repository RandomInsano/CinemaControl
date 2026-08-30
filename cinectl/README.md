# cinectl

Host-side CLI for CinemaControl boards, talking to the firmware over USB
HID via `hidapi`.

## Build

From the repo root: `cargo build -p cinectl` (or just `cargo build`, since
`cinectl` is one of the workspace's `default-members` — see
[AGENTS.md](../AGENTS.md)).

### Linux prerequisites

The `hidapi` crate's default Linux backend (`linux-static-hidraw`) compiles
a small C shim against `libudev` for device enumeration, so `pkg-config` and
libudev's development headers need to be installed before building:

- Debian/Ubuntu: `sudo apt install pkg-config libudev-dev`
- Fedora: `sudo dnf install pkgconf-pkg-config systemd-devel`
- Arch: `sudo pacman -S pkgconf systemd-libs`

No such prerequisite exists on macOS (`hidapi` uses IOHIDManager there, no
external headers needed).

### Linux device permissions

Linux restricts raw HID device access (`/dev/hidraw*`) to root by default.
The `.deb`/`.rpm` packages install [`99-cinemacontrol.rules`](99-cinemacontrol.rules)
to `/usr/lib/udev/rules.d/` automatically, reloading udev on install/removal.
Building from source instead, install it to `/etc/udev/rules.d/` yourself so
a logged-in user can access the board without `sudo`:

    sudo cp 99-cinemacontrol.rules /etc/udev/rules.d/
    sudo udevadm control --reload-rules
    sudo udevadm trigger

Then unplug/replug the board (or reboot) for the rule to take effect. This
relies on `systemd-logind`'s seat-based ACLs (`TAG+="uaccess"`), which
covers every mainstream desktop distro; on a non-systemd system, replace
that tag with a `GROUP="plugdev", MODE="0660"` line matching your distro's
convention instead.

