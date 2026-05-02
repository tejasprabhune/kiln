#!/bin/sh
set -e

REPO="tejasprabhune/kiln"
BINARY="kiln"
INSTALL_DIR="${KILN_INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS and arch
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64)  TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *)      echo "Unsupported macOS architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  Linux)
    # Prefer musl if available for better portability
    if ldd /bin/sh 2>&1 | grep -q musl; then
      LIBC="musl"
    else
      LIBC="gnu"
    fi
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-${LIBC}" ;;
      aarch64) TARGET="aarch64-unknown-linux-musl" ;;
      *)       echo "Unsupported Linux architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS" >&2
    exit 1
    ;;
esac

# Resolve latest release tag if not pinned
if [ -z "$KILN_VERSION" ]; then
  KILN_VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
fi

URL="https://github.com/${REPO}/releases/download/${KILN_VERSION}/kiln-${KILN_VERSION}-${TARGET}.tar.gz"

echo "Installing kiln ${KILN_VERSION} for ${TARGET}"
echo "  from ${URL}"
echo "  into ${INSTALL_DIR}"

mkdir -p "$INSTALL_DIR"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL "$URL" | tar -xz -C "$TMP"
chmod +x "$TMP/$BINARY"
mv "$TMP/$BINARY" "$INSTALL_DIR/$BINARY"

echo ""
echo "kiln installed to ${INSTALL_DIR}/kiln"

# Warn if install dir is not on PATH
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *) echo "  Note: add ${INSTALL_DIR} to your PATH" ;;
esac
