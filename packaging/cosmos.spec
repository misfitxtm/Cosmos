# RPM spec for Cosmos.
#
# NOT TESTED YET: rpmbuild was not available when this was written, so the build, the %check
# and the file list have not been run. The vendored-source step was tested on its own.
#
# To build:
#   sudo dnf install rpm-build cargo-rpm-macros gtk4-devel desktop-file-utils
#   packaging/make-sources.sh ~/rpmbuild/SOURCES
#   rpmbuild -ba packaging/cosmos.spec
#
# Before offering this to Fedora or COPR: list the licences of the bundled crates (see the
# cargo_license_summary macro).

Name:           cosmos
Version:        0.1.0
Release:        1%{?dist}
Summary:        Make a desktop theme from a wallpaper

# The code is GPL. The icon is proprietary but may be used with Cosmos (see
# crates/cosmos-cli/assets/LICENSE-ICON), so Fedora's own repositories would not accept this
# package as it is.
License:        GPL-3.0-or-later AND LicenseRef-Cosmos-icon
URL:            https://github.com/Misfitxtm/Cosmos
Source0:        Cosmos-%{version}.tar.gz
Source1:        Cosmos-%{version}-vendor.tar.xz

BuildRequires:  cargo-rpm-macros
BuildRequires:  gcc
BuildRequires:  pkgconfig(gtk4)
BuildRequires:  desktop-file-utils

%description
Cosmos reads a wallpaper, works out its colours, and turns them into a colour theme
for your desktop. It has a command line tool (cosmos) and a window (cosmos-gtk), and
can write a Hypora theme or fill your own templates.

%prep
%autosetup -n Cosmos-%{version}
tar -xJf %{SOURCE1}
%cargo_prep -v vendor

%build
%{__cargo} build %{__cargo_common_opts} --release --locked -p cosmos-cli -p cosmos-ui

%install
install -Dpm 0755 target/release/cosmos %{buildroot}%{_bindir}/cosmos
install -Dpm 0755 target/release/cosmos-gtk %{buildroot}%{_bindir}/cosmos-gtk
install -Dpm 0644 crates/cosmos-cli/assets/cosmos.svg \
    %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.Misfitxtm.Cosmos.svg
sed 's|@BINDIR@|%{_bindir}|' data/io.github.Misfitxtm.Cosmos.desktop > io.github.Misfitxtm.Cosmos.desktop
desktop-file-install --dir=%{buildroot}%{_datadir}/applications io.github.Misfitxtm.Cosmos.desktop

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/io.github.Misfitxtm.Cosmos.desktop
%{__cargo} test %{__cargo_common_opts} --release --locked -p cosmos-core -p cosmos-out -p cosmos-cli

%files
%license LICENSE crates/cosmos-cli/assets/LICENSE-ICON
%doc README.md AUDIT.md
%{_bindir}/cosmos
%{_bindir}/cosmos-gtk
%{_datadir}/applications/io.github.Misfitxtm.Cosmos.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.Misfitxtm.Cosmos.svg

%changelog
* Fri Oct 23 2026 Misfitxtm <38537232+misfitxtm@users.noreply.github.com> - 0.1.0-1
- First package
