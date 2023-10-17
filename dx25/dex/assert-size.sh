#!/usr/bin/env bash
set -eo pipefail

SIZE=$(stat --format="%s" res/dx25-opt.wasm)
MAX_SIZE=180000
if (( $SIZE > $MAX_SIZE )); then
    echo "Contract binary size $SIZE exceeds $MAX_SIZE limit"
    exit 1
fi
