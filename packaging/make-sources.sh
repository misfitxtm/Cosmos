#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Makes the two tarballs the RPM build needs from the current folder (no git needed):
#   Cosmos-VERSION.tar.gz          the source
#   Cosmos-VERSION-vendor.tar.xz   every dependency, so the build works offline
# Usage: packaging/make-sources.sh [OUTPUT_DIR]   (default: packaging/out)
set -euo pipefail

cd "$(dirname "$(readlink -f "$0")")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
[[ $VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "make-sources.sh: could not read the version" >&2; exit 1; }
OUT=${1:-packaging/out}
mkdir -p "$OUT"
OUT=$(readlink -f "$OUT")
NAME=Cosmos-$VERSION

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/$NAME"
tar --exclude=./target --exclude=./.git --exclude=./vendor --exclude=./packaging/out --exclude=./PLAN.md --exclude=./HYPORA-REPO.md -cf - . | tar -xf - -C "$tmp/$NAME"
tar -C "$tmp" -czf "$OUT/$NAME.tar.gz" "$NAME"

cargo vendor --quiet --locked "$tmp/vendor" >/dev/null
tar -C "$tmp" -cJf "$OUT/$NAME-vendor.tar.xz" vendor
echo "Wrote $OUT/$NAME.tar.gz and $OUT/$NAME-vendor.tar.xz"
