#!/bin/bash
set -eo pipefail
# Builder image version
IMAGE_VER=0.9.0
#
# Locate script's directory and repo's root
#
SELF_DIR=$(readlink -f "$(dirname "$0")")
ROOT_DIR=$(readlink -f "${SELF_DIR}/..")
#
# CLI
#
usage() {
  cat << EOF
Build crate in current directory as either NEAR or Concordium smart contract
using Docker image 'tacanslabs/contract-builder'

Usage:
  $(basename $0) --help|near|concordium|multiversx [opts...]

Where:
  --help      - print this help message and exit with failure

  near        - build as NEAR contract; <opts...> are passed as additional options to 'cargo build'

  concordium  - build as Concordium contract; <opts...> are passed as additional options
                to 'cargo concordium build'; to pass additional parameters to inner 'cargo build',
                specify them after '--' parameter separator
  multiversx  - build as MultiversX contract; <opts...> are passed as additional options to 'cargo build'
EOF
}

case $1 in
  --help)
    usage
    exit 1
  ;;
  near)
    BUILDER=near
    shift
  ;;
  concordium)
    BUILDER=concordium
    shift
  ;;
  multiversx)
    BUILDER=multiversx
    shift
  ;;
  *)
    >&2 echo Invalid option
    usage
    exit 1
  ;;
esac
#
# Run actual builder
#
if [[ ! -e Cargo.toml ]] ; then
  >&2 echo Cargo.toml not found in current working directory, seems it\'s not a crate
  usage
  exit 1
fi

BUILD_PATH=$(realpath --relative-to="${ROOT_DIR}" $(pwd))

docker run --rm \
    -u $(id -u):$(id -g) \
    -v "${ROOT_DIR}":/code \
    tacanslabs/contract-builder:${IMAGE_VER} \
    /build_${BUILDER}.sh ${BUILD_PATH} $@
