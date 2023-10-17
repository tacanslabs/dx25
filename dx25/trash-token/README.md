# Dx25 trash token

## How to build
Use `mxpy` to build the contract:
```bash
mxpy contract build
```

See the [installation guide](https://docs.multiversx.com/sdk-and-tools/sdk-py/installing-mxpy) for how to install the tool.

## How to burn and mint
See `./interaction/localnet-manage.sh` for a list of commands of how to use `mxpy` to make contract calls.

Some notes:
1. Send EGLD when deploying the contract. The SC needs money to issue the token.
2. Pass `baseIssuingCost`  network config value as an argument to the constructor. User can mint only up to `baseIssuingCost`, which currently is `5000000000000000000` for all networks
3. User can burn trash tokens himself, but when burning set your own address as a call receiver.
4. Token ID will always have a random ticker when created. You need to `issue` a trash token. Use issue result, or check ESDT balance to get the ticker value. Also, the management script provides a convenient funtion to get the tiecker and make a localnet REST call to get balance.

```bash
Script usage:
   localnet-manage.sh COMMAND [ARGS]
Commands:
    deploy - Deploy the contract on a localnet
    issue TOKEN_ID NUM_DECIMALS - Issue trash token
    mint TOKEN_ID AMOUNT - Mint AMOUNT tokens
    burn TOKEN_ID AMOUNT - Burn AMOUNT tokens
    tokens - Print issued tokens
    open_balance - Make REST call to the localnet to retrieve balance using a browser
```

## Current trash-token requirements:
| Token Name | Token Ticker | Number of decimals | Verified |
| ---------- | ------------ | ------------------ | -------- |
| Ethereum   | ETH          | 18                 | True     |
| Bitcoin    | BTC          | 8                  | True     |
| Circle USD | USDC         | 6                  | True     |
| Tether USD | USDT         | 6                  | True     |
| Test Token | TRASH        | 18                 | True     |
| Test Token | AQA1         | 6                  | True     |
| Test Token | AQA2         | 18                 | True     |
| Test Token | AQA3         | 12                 | False    |
| Test Token | AQA4         | 18                 | True     |