#!/bin/bash

# set -x

# THis key doesn't matter. Just use registered account key here
ALICE="${HOME}/multiversx-sdk/testwallets/latest/users/alice.pem"
SELF_ADDRESS="$(mxpy wallet pem-address ${ALICE})"
SC_ADDRESS=$(mxpy data load --key=address-testnet)
TX_RESULT_LOCATION="/tmp/dx25-trash-token-localnet.interaction.json"
PROXY=http://localhost:7950

deploy() {
    mxpy contract build && mxpy --verbose contract deploy --project=. --pem="${ALICE}" --proxy=${PROXY} --gas-limit=600000000 --recall-nonce --send --value 1000000000000000000000 --metadata-payable-by-sc --arguments 5000000000000000000 --outfile="${TX_RESULT_LOCATION}" || return

    local SC_ADDRESS=$(mxpy data parse --file="${TX_RESULT_LOCATION}" --expression="data['contractAddress']")

    mxpy data store --key=address-testnet --value=${SC_ADDRESS}

    echo ""
    echo "Smart contract address: ${SC_ADDRESS}"
}

issue() {
    if [ "$#" -ne 3 ];
    then
        echo "Pass token TICKER, token NAME, and num of decimals"
        exit 1
    fi

    mxpy --verbose contract call ${SC_ADDRESS} --recall-nonce --gas-limit=60000000 --function=issue --arguments "str:$1" "str:$2" $3 --pem="${ALICE}" --proxy=${PROXY} --send --wait-result || return
}

mint() {
    if [ "$#" -ne 2 ];
    then
        echo "Pass token TICKER and amount"
        exit 1
    fi

    mxpy --verbose contract call ${SC_ADDRESS} --recall-nonce --gas-limit=60000000 --function=mint --arguments "str:$1" $2 --pem="${ALICE}" --proxy=${PROXY} --send --wait-result || return
}

burn() {
    if [ "$#" -ne 2 ];
    then
        echo "Pass token TICKER and amount"
        exit 1
    fi

    # Note, we use self adress as a receiver
    mxpy --verbose contract call ${SELF_ADDRESS} --recall-nonce --gas-limit=300000 --function=ESDTLocalBurn --arguments "str:$1" $2 --pem="${ALICE}" --proxy=${PROXY} --send --wait-result || return
}

tokens() {
    mxpy --verbose contract query ${SC_ADDRESS} --function=tokens  > "${TX_RESULT_LOCATION}" || return

    for row in $(cat "${TX_RESULT_LOCATION}" | jq -r '.[].hex'); do
        echo ${row} | xxd -r -p
        echo ""
    done
}

open_balance() {
    xdg-open "http://localhost:7950/address/${SELF_ADDRESS}/esdt"
}

print_help() {
    echo -e "\nUsage:\n   localnet-manage.sh [COMMAND] [ARGS]"
    echo -e "Commands:"
    echo -e "    deploy - Deploy the contract on a localnet"
    echo -e "    issue TOKEN_NAME TOKEN_ID NUM_DECIMALS - Issue trash token"
    echo -e "    mint TOKEN_ID AMOUNT - Mint AMOUNT tokens"
    echo -e "    burn TOKEN_ID AMOUNT - Burn AMOUNT tokens"
    echo -e "    tokens - Print issued tokens"
    echo -e "    open_balance - Make REST call to the localnet to retrieve balance using a browser"
}

case "$1" in
    "") print_help; exit;;
    deploy) "$@"; exit;;
    issue) "$@"; exit;;
    mint) "$@"; exit;;
    burn) "$@"; exit;;
    tokens) "$@"; exit;;
    open_balance) "$@"; exit;;
    *) echo "Unknown function: $1()"; print_help; exit 2;;
esac