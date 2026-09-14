#!/usr/bin/env bash
# Fetches the bundled llama.cpp runtime into src-tauri/binaries/llama on macOS
# and Linux. The Windows counterpart is fetch-llama.ps1; the build is pinned to
# the same release there, so every platform ships the one that was tested.
#
# macOS gets the Metal build, Linux the Vulkan one: both fall back to CPU when
# no usable device is found, as the Windows Vulkan build does.

set -euo pipefail

build='b10897'

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)  asset="llama-$build-bin-macos-arm64.tar.gz" ;;
  Darwin-x86_64) asset="llama-$build-bin-macos-x64.tar.gz" ;;
  Linux-x86_64)  asset="llama-$build-bin-ubuntu-vulkan-x64.tar.gz" ;;
  Linux-aarch64) asset="llama-$build-bin-ubuntu-vulkan-arm64.tar.gz" ;;
  *) echo "No pinned llama.cpp build for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
dest="$root/src-tauri/binaries/llama"

if [ -f "$dest/llama-server" ]; then
  echo "Already present at $dest. Delete it to re-fetch."
  exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "Fetching $asset ..."
curl -fsSL "https://github.com/ggml-org/llama.cpp/releases/download/$build/$asset" -o "$work/$asset"
tar -xzf "$work/$asset" -C "$work"

mkdir -p "$dest"
src="$(find "$work" -type f -name llama-server -print -quit)"
[ -n "$src" ] || { echo "llama-server not found in $asset" >&2; exit 1; }
dir="$(dirname "$src")"

# Every shared library, dereferenced: the archive links versioned names to one
# file, and a bundler copying the glob is not guaranteed to keep symlinks.
# Only llama-server among the executables.
cp -L "$src" "$dest/"
find "$dir" -maxdepth 1 \( -name '*.dylib' -o -name '*.so' -o -name '*.so.*' \) -print0 |
  while IFS= read -r -d '' lib; do cp -L "$lib" "$dest/"; done
chmod +x "$dest/llama-server"

echo "Installed llama-server and $(find "$dest" -maxdepth 1 \( -name '*.dylib' -o -name '*.so*' \) | wc -l | tr -d ' ') libraries to $dest"
