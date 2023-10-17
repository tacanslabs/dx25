# EGLD-WEGLD swap

## Overview

The EGLD-WEGLD swap contract mints and distributes the WEGLD token, in equal amount to the amount of EGLD locked in the contract.

There are such contracts deployed in each shard.

Coplete copy from `mx-sdk-rs` repository

## How to build
Use `mxpy` to build the contract:
```bash
mxpy contract build
```

See the [installation guide](https://docs.multiversx.com/sdk-and-tools/sdk-py/installing-mxpy) for how to install the tool.

## How to deploy
See `./interaction/localnet-manage.sh` for a list of commands of how to use `mxpy` to make contract calls.

Note: token ID will always have a random ticker when created. One can use `wrappedEgldTokenId` to read token ID. Don't forget to update `localnet-manage.sh` `${WRAPPED_EGLD_TOKEN_ID}` after issuance if you want to wrpa/unwrap manually.

Perform following actions to fully deploy the contract:
1. `issueWrappedEgld` (skip this one if you already have token ID)
2. Open `http://localhost:7950/address/erd1qyu5wthldzr8wx5c9ucg8kjagg0jfs53s8nr3zpz3hypefsdd8ssycr6th/esdt` to check wrapped token ID
3. Update `${WRAPPED_EGLD_TOKEN_ID}` var in the script if you've ran step `2`. Can have `str` fromat. For example `str:WEGLD-dc1a0a`
4. `deploy`
5. `setLocalRoles`

