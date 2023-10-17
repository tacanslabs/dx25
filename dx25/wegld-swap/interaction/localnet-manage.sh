#!/bin/bash

set -x

ALICE="${HOME}/multiversx-sdk/testwallets/latest/users/alice.pem"
BOB="${HOME}/multiversx-sdk/testwallets/latest/users/bob.pem"
ADDRESS=$(mxpy data load --key=address-testnet)
TX_RESULT_LOCATION="/tmp/dx25-swap-egld-localnet.interaction.json"
PROXY=http://localhost:7950

ESDT_SYSTEM_SC_ADDRESS=erd1qqqqqqqqqqqqqqqpqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqzllls8a5w6u

######################################################################
############################ Update after issue ######################
######################################################################
WRAPPED_EGLD_TOKEN_ID="str:WEGLD-***"

deploy() {
    mxpy --verbose contract deploy --project=${PROJECT} --recall-nonce --pem=${ALICE} \
    --gas-limit=100000000 \
    --arguments ${WRAPPED_EGLD_TOKEN_ID} \
    --send --outfile="${TX_RESULT_LOCATION}" --proxy=${PROXY} || return

    ADDRESS=$(mxpy data parse --file="${TX_RESULT_LOCATION}" --expression="data['contractAddress']")

    mxpy data store --key=address-testnet --value=${ADDRESS}

    echo ""
    echo "Smart contract address: ${ADDRESS}"
}

upgrade() {
    mxpy --verbose contract upgrade ${ADDRESS} --project=${PROJECT} --recall-nonce --pem=${ALICE} \
    --arguments ${WRAPPED_EGLD_TOKEN_ID} --gas-limit=100000000 --outfile="upgrade.json" \
    --send --proxy=${PROXY} || return
}

issueWrappedEgld() {
    local TOKEN_DISPLAY_NAME=0x5772617070656445676c64  # "WrappedEgld"
    local TOKEN_TICKER=0x5745474c44  # "WEGLD"
    local INITIAL_SUPPLY=0x01 # 1
    local NR_DECIMALS=0x12 # 18
    local CAN_ADD_SPECIAL_ROLES=0x63616e4164645370656369616c526f6c6573 # "canAddSpecialRoles"
    local TRUE=0x74727565 # "true"

    mxpy --verbose contract call ${ESDT_SYSTEM_SC_ADDRESS} --recall-nonce --pem=${ALICE} \
    --gas-limit=60000000 --value=5000000000000000000 --function="issue" \
    --arguments ${TOKEN_DISPLAY_NAME} ${TOKEN_TICKER} ${INITIAL_SUPPLY} ${NR_DECIMALS} ${CAN_ADD_SPECIAL_ROLES} ${TRUE} \
    --send --proxy=${PROXY} --wait-result

    xdg-open "http://${PROXY}/address/erd1qyu5wthldzr8wx5c9ucg8kjagg0jfs53s8nr3zpz3hypefsdd8ssycr6th/esdt"
}

setLocalRoles() {
    mxpy --verbose contract call ${ESDT_SYSTEM_SC_ADDRESS} --recall-nonce --pem=${ALICE} \
    --gas-limit=60000000 --function="setSpecialRole" \
    --arguments ${WRAPPED_EGLD_TOKEN_ID} ${ADDRESS} "str:ESDTRoleLocalMint" "str:ESDTRoleLocalBurn" \
    --send --proxy=${PROXY} --wait-result
}

wrapEgldBob() {
    mxpy --verbose contract call ${ADDRESS} --recall-nonce --pem=${BOB} \
    --gas-limit=10000000 --value=1000 --function="wrapEgld" \
    --send --proxy=${PROXY}  --wait-result
}

unwrapEgldBob() {
    mxpy --verbose contract call ${ADDRESS} --recall-nonce --pem=${BOB} \
    --gas-limit=10000000 --function="ESDTTransfer" \
    --arguments ${WRAPPED_EGLD_TOKEN_ID} 1000 "str:unwrapEgld" \
    --send --proxy=${PROXY} --wait-result
}

getWrappedEgldTokenIdentifier() {
    local QUERY_OUTPUT=$(mxpy --verbose contract query ${ADDRESS} --function="getWrappedEgldTokenId" --proxy=${PROXY})
    TOKEN_IDENTIFIER=0x$(jq -r '.[0] .hex' <<< "${QUERY_OUTPUT}")
    echo "Wrapped eGLD token identifier: ${TOKEN_IDENTIFIER}"
}

getLockedEgldBalance() {
    mxpy --verbose contract query ${ADDRESS} --function="getLockedEgldBalance" --proxy=${PROXY}
}

case "$1" in
    "") print_help; exit;;
    deploy) "$@"; exit;;
    issueWrappedEgld) "$@"; exit;;
    setLocalRoles) "$@"; exit;;
    wrapEgldBob) "$@"; exit;;
    unwrapEgldBob) "$@"; exit;;
    getWrappedEgldTokenIdentifier) "$@"; exit;;
    getLockedEgldBalance) "$@"; exit;;
    *) echo "Unknown function: $1()"; print_help; exit 2;;
esac
