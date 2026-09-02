%global debug_package %{nil}
%{!?_udevrulesdir: %global _udevrulesdir %{_prefix}/lib/udev/rules.d}

Name:           cinectl
Version:        %{?version}%{!?version:0.0.0}
Release:        1%{?dist}
Summary:        %{?summary}%{!?summary:Host-side CLI for CinemaControl boards}

License:        MIT
URL:            https://github.com/RandomInsano/CinemaControl

Requires:       systemd-udev

# %%{workspace_dir}/target/release/cinectl is expected to already be built
# (by the cinectl-linux CI job, downloaded into place before rpmbuild runs)
# rather than compiled by this spec, so there's no %%prep/%%build here — a
# real Fedora-style SRPM rebuild would need to add cargo/rust BuildRequires
# and a %%build section invoking `cargo build --release -p cinectl` itself.

# Named pkg_description, not description: `--define "description ..."`
# would shadow rpm's own %description section marker, since macro
# expansion runs before rpm recognizes section headers.
%description
%{?pkg_description}%{!?pkg_description:Host-side CLI for CinemaControl boards, talking to the firmware over USB HID.}

%install
install -Dm755 %{workspace_dir}/target/release/cinectl %{buildroot}%{_bindir}/cinectl
strip %{buildroot}%{_bindir}/cinectl
install -Dm644 %{workspace_dir}/cinectl/packaging/99-cinemacontrol.rules %{buildroot}%{_udevrulesdir}/99-cinemacontrol.rules
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
