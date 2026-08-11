#!/usr/bin/env bash
set -euo pipefail

readonly version="1.7.12"
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)
    readonly archive="actionlint_${version}_darwin_arm64.tar.gz"
    readonly expected="aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f"
    ;;
  Darwin-x86_64)
    readonly archive="actionlint_${version}_darwin_amd64.tar.gz"
    readonly expected="5b44c3bc2255115c9b69e30efc0fecdf498fdb63c5d58e17084fd5f16324c644"
    ;;
  Linux-x86_64)
    readonly archive="actionlint_${version}_linux_amd64.tar.gz"
    readonly expected="8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8"
    ;;
  Linux-aarch64|Linux-arm64)
    readonly archive="actionlint_${version}_linux_arm64.tar.gz"
    readonly expected="325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6"
    ;;
  *)
    echo "Unsupported actionlint host: $(uname -s)-$(uname -m)" >&2
    exit 2
    ;;
esac

readonly temporary="$(mktemp -d "${TMPDIR:-/tmp}/quisum-actionlint.XXXXXX")"
cleanup() {
  rm -rf -- "$temporary"
}
trap cleanup EXIT

curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
  "https://github.com/rhysd/actionlint/releases/download/v${version}/${archive}" \
  --output "$temporary/$archive"

if command -v shasum >/dev/null 2>&1; then
  readonly actual="$(shasum -a 256 "$temporary/$archive" | awk '{print $1}')"
else
  readonly actual="$(sha256sum "$temporary/$archive" | awk '{print $1}')"
fi
test "$actual" = "$expected"

tar -xzf "$temporary/$archive" -C "$temporary"
"$temporary/actionlint" .github/workflows/ci.yml .github/workflows/release.yml
