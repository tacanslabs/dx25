#!/bin/bash
set -eo pipefail
CRATE_DIR="$( cd -- "$(dirname "$0")" >/dev/null 2>&1 ; pwd -P )"

# Optional cargo args
BUILD_ARGS="$@"

pushd ${CRATE_DIR}/meta >/dev/null

# We run build scrips manually, not using mxpy, because we want to lock rustc version.
# mxpy doesn't allow to do this.
# See official reference for more details: https://docs.elrond.com/developers/developer-reference/smart-contract-build-reference/
cargo run build --target=wasm32-unknown-unknown  ${BUILD_ARGS}

popd >/dev/null
