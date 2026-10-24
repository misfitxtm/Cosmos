# RPM spec for Cosmos.
#
# Built and checked on 2026-10-23: rpmbuild and mock (Fedora 44, no network) both succeed, the
# tests in %%check pass, and rpmlint reports no errors. Not yet tried: installing the result
# with dnf on a separate clean machine.
#
# To build:
#   sudo dnf install rpm-build cargo-rpm-macros gtk4-devel desktop-file-utils
#   packaging/make-sources.sh ~/rpmbuild/SOURCES   (or let rpmbuild fetch them: spectool -g -R)
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
# package as it is. The rest is the licences of the Rust crates compiled into the programs;
# it comes from the cargo_license_summary macro, which the build prints. Recheck it whenever
# the dependencies change.
License:        GPL-3.0-or-later AND LicenseRef-Cosmos-icon AND MIT AND (MIT OR Apache-2.0) AND (0BSD OR MIT OR Apache-2.0) AND (BSD-3-Clause OR Apache-2.0) AND (MIT OR Apache-2.0 OR Zlib) AND (Unlicense OR MIT)
URL:            https://github.com/Misfitxtm/Cosmos
# Source0 is GitHub's archive of the v%%{version} tag. Source1 (every dependency, so the build
# works offline) is attached to the same GitHub release; packaging/make-sources.sh makes both.
Source0:        %{url}/archive/v%{version}/Cosmos-%{version}.tar.gz
Source1:        %{url}/releases/download/v%{version}/Cosmos-%{version}-vendor.tar.xz

BuildRequires:  cargo-rpm-macros
BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  pkgconfig(gtk4)
BuildRequires:  desktop-file-utils

%description
Cosmos reads a wallpaper, works out its colors, and turns them into a color
theme for your desktop. It comes with a command line tool and a window, and it
can write a Hypora theme or fill in your own templates.

%prep
%autosetup -n Cosmos-%{version}
tar -xJf %{SOURCE1}
%cargo_prep -v vendor

%build
%cargo_build -- -p cosmos-cli -p cosmos-ui
# The binaries contain every dependency, so their licences travel with the package
%{cargo_license_summary}
%{cargo_license} > LICENSE.dependencies
%{cargo_vendor_manifest}

%install
install -Dpm 0755 target/release/cosmos %{buildroot}%{_bindir}/cosmos
install -Dpm 0755 target/release/cosmos-gtk %{buildroot}%{_bindir}/cosmos-gtk
install -Dpm 0644 crates/cosmos-cli/assets/cosmos.svg \
    %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.Misfitxtm.Cosmos.svg
sed 's|@BINDIR@|%{_bindir}|' data/io.github.Misfitxtm.Cosmos.desktop > io.github.Misfitxtm.Cosmos.desktop
desktop-file-install --dir=%{buildroot}%{_datadir}/applications io.github.Misfitxtm.Cosmos.desktop

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/io.github.Misfitxtm.Cosmos.desktop
%{__cargo} test %{__cargo_common_opts} --profile rpm --no-fail-fast -p cosmos-core -p cosmos-out -p cosmos-cli

%files
%license LICENSE crates/cosmos-cli/assets/LICENSE-ICON LICENSE.dependencies cargo-vendor.txt
%doc README.md AUDIT.md
%{_bindir}/cosmos
%{_bindir}/cosmos-gtk
%{_datadir}/applications/io.github.Misfitxtm.Cosmos.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.Misfitxtm.Cosmos.svg

%changelog
* Fri Oct 23 2026 Misfitxtm <38537232+misfitxtm@users.noreply.github.com> - 0.1.0-1
- First package
