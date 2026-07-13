#!/usr/bin/env bash
# Install a published relay-sync macOS release without requiring Rust.
set -euo pipefail

REPOSITORY="${RELAY_SYNC_REPOSITORY:-vgseven/sync}"
INSTALL_DIR="${RELAY_SYNC_INSTALL_DIR:-$HOME/.local/bin}"
REQUESTED_VERSION="${RELAY_SYNC_VERSION:-latest}"
API_URL="https://api.github.com/repos/${REPOSITORY}/releases/latest"

fail() {
  printf 'relay-sync installer: %s\n' "$*" >&2
  exit 1
}

download() {
  local url="$1"
  local destination="$2"

  if command -v curl >/dev/null 2>&1; then
    curl --fail --location --silent --show-error --retry 3 --output "$destination" "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget --quiet --output-document="$destination" "$url"
  else
    fail "curl or wget is required"
  fi
}

sha256() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    fail "shasum or sha256sum is required"
  fi
}

resolve_version() {
  if [[ "$REQUESTED_VERSION" != "latest" ]]; then
    printf '%s\n' "${REQUESTED_VERSION#v}"
    return
  fi

  local tag
  download "$API_URL" "${work_dir}/latest-release.json"
  tag="$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "${work_dir}/latest-release.json" | head -n 1)"
  [[ -n "$tag" ]] || fail "could not determine the latest release version"
  printf '%s\n' "${tag#v}"
}

case "$(uname -s)" in
  Darwin) ;;
  *) fail "this installer currently supports macOS only" ;;
esac

case "$(uname -m)" in
  arm64|aarch64) target="aarch64-apple-darwin" ;;
  x86_64) target="x86_64-apple-darwin" ;;
  *) fail "unsupported macOS architecture: $(uname -m)" ;;
esac

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
version="$(resolve_version)"
tag="v${version}"
archive="relay-sync-${version}-${target}.tar.gz"
checksum_file="${archive}.sha256"
release_url="https://github.com/${REPOSITORY}/releases/download/${tag}"

printf 'Installing relay-sync %s for %s...\n' "$tag" "$target"
download "${release_url}/${archive}" "${work_dir}/${archive}"
download "${release_url}/${checksum_file}" "${work_dir}/${checksum_file}"

expected="$(awk -v archive="$archive" '$2 == archive { print $1; exit }' "${work_dir}/${checksum_file}")"
actual="$(sha256 "${work_dir}/${archive}")"
[[ -n "$expected" ]] || fail "checksum file does not contain ${archive}"
[[ "$expected" == "$actual" ]] || fail "checksum verification failed"

tar -xzf "${work_dir}/${archive}" -C "$work_dir"
binary="${work_dir}/relay-sync-${version}-${target}/relay-sync"
[[ -f "$binary" ]] || fail "release archive does not contain relay-sync"

mkdir -p "$INSTALL_DIR"
install -m 755 "$binary" "${INSTALL_DIR}/relay-sync"

printf 'Installed relay-sync %s to %s\n' "$tag" "${INSTALL_DIR}/relay-sync"
case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *) printf 'Add this directory to PATH: export PATH="%s:$PATH"\n' "$INSTALL_DIR" ;;
esac
