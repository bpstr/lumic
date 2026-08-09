#!/usr/bin/env sh
set -eu

: "${LUMIC_TEST_BINARY:?set LUMIC_TEST_BINARY to the mounted static lumic binary}"
export LUMIC_INSTALL_BINARY="$LUMIC_TEST_BINARY"
export LUMIC_INSTALL_DIR="/usr/local/bin"

/install.sh
lumic version
lumic status

# Installer should be idempotent for the same binary.
/install.sh
lumic version
