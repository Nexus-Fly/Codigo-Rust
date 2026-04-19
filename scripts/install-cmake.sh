#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# install-cmake.sh
# Downloads CMake 4.3.1 from the official Kitware GitHub release and installs
# it into /opt/cmake, then symlinks the binaries to /usr/local/bin.
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

CMAKE_VERSION="4.3.1"
CMAKE_ARCH="linux-x86_64"
CMAKE_DIR="cmake-${CMAKE_VERSION}-${CMAKE_ARCH}"
CMAKE_TARBALL="${CMAKE_DIR}.tar.gz"
DOWNLOAD_URL="https://github.com/Kitware/CMake/releases/download/v${CMAKE_VERSION}/${CMAKE_TARBALL}"
INSTALL_PREFIX="/opt/cmake"

echo "==> Downloading CMake ${CMAKE_VERSION}..."
curl -fsSL -o "/tmp/${CMAKE_TARBALL}" "${DOWNLOAD_URL}"

echo "==> Extracting..."
tar -xzf "/tmp/${CMAKE_TARBALL}" -C /tmp

echo "==> Installing to ${INSTALL_PREFIX}..."
mkdir -p "${INSTALL_PREFIX}"
cp -r "/tmp/${CMAKE_DIR}/." "${INSTALL_PREFIX}/"

echo "==> Symlinking binaries to /usr/local/bin..."
ln -sf "${INSTALL_PREFIX}/bin/cmake"  /usr/local/bin/cmake
ln -sf "${INSTALL_PREFIX}/bin/ctest"  /usr/local/bin/ctest
ln -sf "${INSTALL_PREFIX}/bin/cpack"  /usr/local/bin/cpack

echo "==> Cleaning up..."
rm -f "/tmp/${CMAKE_TARBALL}"
rm -rf "/tmp/${CMAKE_DIR}"

echo "==> Verifying installation:"
cmake --version
