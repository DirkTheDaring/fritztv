#!/usr/bin/env bash
set -euo pipefail

OUT_DIR=${1:-/out}
mkdir -p "$OUT_DIR"

# Determine package metadata from Cargo.toml (avoid extra deps like jq).
NAME=$(grep -E '^name\s*=\s*"' Cargo.toml | head -n1 | sed -E 's/^name\s*=\s*"([^"]+)".*/\1/')
VERSION=$(grep -E '^version\s*=\s*"' Cargo.toml | head -n1 | sed -E 's/^version\s*=\s*"([^"]+)".*/\1/')
ARCH=$(dpkg --print-architecture)

if [[ -z "$NAME" || -z "$VERSION" ]]; then
  echo "Failed to parse name/version from Cargo.toml" >&2
  exit 1
fi

# Build release binary.
cargo build --release

# Allow external override of build directory (e.g. for inspection in target/debianbuild)
BUILD_DIR=${BUILD_DIR:-/tmp}
PKGROOT="$BUILD_DIR/${NAME}_${VERSION}_${ARCH}"
rm -rf "$PKGROOT"

# Filesystem layout
install -D -m 0755 "target/release/${NAME}" "$PKGROOT/usr/bin/${NAME}"
install -D -m 0644 "packaging/rpm/fritztv.service" "$PKGROOT/lib/systemd/system/${NAME}.service"
install -D -m 0644 "config.toml" "$PKGROOT/etc/${NAME}/config.toml"
install -D -m 0644 "packaging/rpm/fritztv.sysconfig" "$PKGROOT/etc/default/${NAME}"

# Docs
install -D -m 0644 "LICENSE" "$PKGROOT/usr/share/doc/${NAME}/copyright"

# Minimal Debian changelog (not strict, but nice to have)
mkdir -p "$PKGROOT/usr/share/doc/${NAME}"
echo "${NAME} (${VERSION}) unstable; urgency=low" > "$PKGROOT/usr/share/doc/${NAME}/changelog.Debian"
echo >> "$PKGROOT/usr/share/doc/${NAME}/changelog.Debian"
echo "  * Automated build." >> "$PKGROOT/usr/share/doc/${NAME}/changelog.Debian"
echo >> "$PKGROOT/usr/share/doc/${NAME}/changelog.Debian"
echo " -- ${NAME} packaging <noreply@localhost>  $(date -R)" >> "$PKGROOT/usr/share/doc/${NAME}/changelog.Debian"
gzip -9 -n "$PKGROOT/usr/share/doc/${NAME}/changelog.Debian"

# DEBIAN control and maintainer scripts
mkdir -p "$PKGROOT/DEBIAN"
cat > "$PKGROOT/DEBIAN/control" <<EOF
Package: ${NAME}
Version: ${VERSION}
Section: video
Priority: optional
Architecture: ${ARCH}
Maintainer: ${NAME} packaging <noreply@localhost>
Depends: ffmpeg, adduser, systemd
Description: Fritztv Transcoding Server for FritzBox Cable
 A transcoding server that interfaces with FritzBox Cable TV tuners to provide
 browser-compatible streams via RTSP to fMP4/HLS.
EOF

cat > "$PKGROOT/DEBIAN/postinst" <<'EOF'
#!/usr/bin/env sh
set -e

# Create service user/group if missing.
if ! getent group fritztv >/dev/null 2>&1; then
  addgroup --system fritztv >/dev/null 2>&1 || true
fi
if ! getent passwd fritztv >/dev/null 2>&1; then
  adduser --system --ingroup fritztv --home /var/lib/fritztv --shell /usr/sbin/nologin --gecos "Fritztv Service User" fritztv >/dev/null 2>&1 || true
fi

# Ensure state dir exists.
mkdir -p /var/lib/fritztv
chown -R fritztv:fritztv /var/lib/fritztv || true
chmod 0750 /var/lib/fritztv || true

# Reload unit files if systemd is present.
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload >/dev/null 2>&1 || true
fi

exit 0
EOF
chmod 0755 "$PKGROOT/DEBIAN/postinst"

cat > "$PKGROOT/DEBIAN/prerm" <<'EOF'
#!/usr/bin/env sh
set -e

if command -v systemctl >/dev/null 2>&1; then
  systemctl stop fritztv.service >/dev/null 2>&1 || true
fi

exit 0
EOF
chmod 0755 "$PKGROOT/DEBIAN/prerm"

# Build .deb
DEB_PATH="$OUT_DIR/${NAME}_${VERSION}_${ARCH}.deb"
rm -f "$DEB_PATH"
dpkg-deb --build "$PKGROOT" "$DEB_PATH" >/dev/null

echo "Built: $DEB_PATH"
