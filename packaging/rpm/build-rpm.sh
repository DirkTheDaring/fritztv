#!/usr/bin/env bash
set -euo pipefail

OUT_DIR=${1:-/out}
RPMBUILD_DIR=/root/rpmbuild

echo "Building RPM for Fedora..."

# Determine version
VERSION=$(grep "^version =" Cargo.toml | head -n1 | cut -d '"' -f 2)
PROJECT="fritztv"

# Prepare directories
mkdir -p "$RPMBUILD_DIR"/{SOURCES,SPECS,BUILD,RPMS,SRPMS}

# Create source tarball from mounted source
echo "Creating source tarball..."
git archive --format=tar.gz --prefix=${PROJECT}-${VERSION}/ -o "${RPMBUILD_DIR}/SOURCES/${PROJECT}-${VERSION}.tar.gz" HEAD

# Copy auxiliary files
cp config.toml "${RPMBUILD_DIR}/SOURCES/"
cp packaging/rpm/fritztv.service packaging/rpm/fritztv.sysconfig packaging/rpm/fritztv.sysusers "${RPMBUILD_DIR}/SOURCES/"

# Copy and update spec file
cp packaging/rpm/fritztv.spec "${RPMBUILD_DIR}/SPECS/"
sed -i "s/^Version:.*/Version:        ${VERSION}/" "${RPMBUILD_DIR}/SPECS/fritztv.spec"

# Build
echo "Running rpmbuild..."
rpmbuild -ba "${RPMBUILD_DIR}/SPECS/fritztv.spec"

# Copy output
echo "Copying artifacts to $OUT_DIR..."
mkdir -p "$OUT_DIR"
find "${RPMBUILD_DIR}/RPMS" -name "*.rpm" -exec cp {} "$OUT_DIR/" \;
find "${RPMBUILD_DIR}/SRPMS" -name "*.rpm" -exec cp {} "$OUT_DIR/" \;

echo "Done."
