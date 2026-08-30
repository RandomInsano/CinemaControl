%global debug_package %{nil}
%{!?_udevrulesdir: %global _udevrulesdir %{_prefix}/lib/udev/rules.d}

Name:           cinectl
Version:        %{?version}%{!?version:0.0.0}
Release:        1%{?dist}
Summary:        %{?summary}%{!?summary:Host-side CLI for CinemaControl boards}

License:        MIT
URL:            https://github.com/RandomInsano/CinemaControl

BuildRequires:  cargo, rust, pkgconfig(libudev)
Requires:       systemd-udev

# Built straight from the workspace passed in via `--define "workspace_dir
# ..."` rather than a Source0/%prep tarball round trip: this spec only ever
# runs from CI against a git checkout, not a Fedora-style SRPM rebuild.

%description
%{?description}%{!?description:Host-side CLI for CinemaControl boards, talking to the firmware over USB HID.}

%build
cd %{workspace_dir}
cargo build --release -p cinectl

%install
install -Dm755 %{workspace_dir}/target/release/cinectl %{buildroot}%{_bindir}/cinectl
strip %{buildroot}%{_bindir}/cinectl
install -Dm644 %{workspace_dir}/cinectl/99-cinemacontrol.rules %{buildroot}%{_udevrulesdir}/99-cinemacontrol.rules
install -Dm644 %{workspace_dir}/cinectl/README.md %{buildroot}%{_docdir}/%{name}/README.md

%files
%{_bindir}/cinectl
%{_udevrulesdir}/99-cinemacontrol.rules
%doc %{_docdir}/%{name}/README.md

%post
udevadm control --reload-rules >/dev/null 2>&1 || :
udevadm trigger >/dev/null 2>&1 || :

%postun
udevadm control --reload-rules >/dev/null 2>&1 || :

%changelog
* %{?changelog_date}%{!?changelog_date:Sun Jan 01 2026} Edwin Amsler <EdwinGuy@GMail.com> - %{version}-1
- Packaging build for cinectl %{version}.
