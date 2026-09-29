# Devnet Deployment

## Current deployment

| Property                      | Value                                          |
| ----------------------------- | ---------------------------------------------- |
| Cluster                       | Solana devnet                                  |
| Program ID                    | `AG4ztZvcZjzFKjoXLzH8M6U3if9CwVFTXH2FHjqg7SdL` |
| ProgramData address           | `A4zPViwdWBgB7ttkEg5dGqumXJ7ZygogEwVHgUb3VBrY` |
| Upgrade authority             | `BX6RJHGbi7msj7t1ECCX6T1ZvvetHDK6UkjzAhPfWngq` |
| Last verified deployment slot | `505480906`                                    |

[Open the program in Solana Explorer](https://explorer.solana.com/address/AG4ztZvcZjzFKjoXLzH8M6U3if9CwVFTXH2FHjqg7SdL?cluster=devnet).

## Deploying updates

Build and verify before every upgrade:

```bash
anchor build
anchor test --skip-deploy
anchor program deploy --no-idl
```

Bare `anchor test` deploys to the provider cluster before executing the test
script. Use `--skip-deploy` for the LiteSVM-only local verification workflow.

The deployment keypair at `target/deploy/tidebook-keypair.json` determines the
program address. It is intentionally excluded from Git and must be preserved
securely; replacing it would create a different program ID.

## Public test-faucet bootstrap

The faucet feature must be deployed before the UI can initialize or claim from
it. Use fresh legacy SPL Token mints whose supply is zero and whose mint
authority is the super-admin. Never attach the faucet to real assets: the
program cannot detect its cluster and claims are intentionally unlimited.

Recommended first research market:

| Setting            | Human value   | On-chain value                      |
| ------------------ | ------------- | ----------------------------------- |
| Pair               | `mSOL/mUSD`   | fresh base and quote mint addresses |
| Base decimals      | 9             | mint configuration                  |
| Quote decimals     | 6             | mint configuration                  |
| Price tick         | `0.01 mUSD`   | `10_000` quote atoms                |
| Quantity lot       | `0.01 mSOL`   | `10_000_000` base atoms             |
| Faucet base claim  | `10 mSOL`     | `10_000_000_000` base atoms         |
| Faucet quote claim | `10,000 mUSD` | `10_000_000_000` quote atoms        |
| Example first bid  | `149.50 mUSD` | entered as `149.50` in the UI       |
| Example first ask  | `150.50 mUSD` | entered as `150.50` in the UI       |

Bootstrap order:

1. Deploy the rebuilt program and web app with the same program ID.
2. Create fresh `mSOL` and `mUSD` legacy SPL mints with 9 and 6 decimals.
3. Do not mint any supply before faucet initialization.
4. Create the market with tick `10000` and lot `10000000`.
5. Enable the faucet as super-admin with claims `10` and `10000`; this
   transfers both mint authorities to the faucet PDA.
6. Any connected wallet can claim, initialize its internal balance, and
   deposit the amount it wants to trade.
7. Enter prices and quantities in human units such as `150.50` and `0.01`.

Record every resulting public address and transaction in
`docs/devnet-addresses.md`. Closing this market also closes the faucet config
and permanently stops further minting for that pair.

## IDL metadata limitation

The first `anchor deploy` invocation successfully deployed the executable but
failed during the separate IDL metadata step. Anchor invokes the latest
`@solana-program/program-metadata` CLI through `npx`. Version `0.9.4` requires
`@solana/kit` as a peer dependency, but it was absent from the temporary npx
environment, producing `Cannot find module '@solana/kit'`.

The program deployment itself is valid and independently verified with
`solana program show`. Until the upstream packaging path is fixed, upgrades use
`--no-idl`, and the frontend uses the IDL generated locally by `anchor build`.

No matching open issue was found in the program-metadata repository at the time
of the initial deployment. This is a candidate for a focused upstream report or
contribution after a minimal standalone reproduction is prepared.
