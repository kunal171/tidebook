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

The web faucet uses a dedicated server-side devnet keypair as the mint authority
for one configured pair. The route mints fixed amounts directly to the
requester's associated token accounts and pays their creation rent when needed.
Never use this design for valuable or mainnet assets.

Required server-only variables are listed in `app/.env.example`. Store
`FAUCET_AUTHORITY_SECRET_KEY` only in encrypted Vercel environment settings.
Vercel also requires `UPSTASH_REDIS_REST_URL` and
`UPSTASH_REDIS_REST_TOKEN`; without them the endpoint reports itself disabled
and rejects claims. `FAUCET_ALLOW_IN_MEMORY_RATE_LIMIT=true` is permitted only
for local testing.

Bootstrap order:

1. Generate a dedicated keypair and fund it with a small amount of devnet SOL.
2. Configure the fixed mint addresses, atomic claim amounts, and claim window.
3. Transfer both devnet mint authorities to the dedicated public key.
4. Configure durable Upstash credentials in Vercel.
5. Deploy the web application and open a market using that exact ordered mint
   pair.
6. Connect a wallet and use **Claim test tokens**. The server creates missing
   associated token accounts and atomically mints both configured assets.
7. Confirm that a repeated request is rejected with HTTP `429`.

The current shared pair, public authority, and authority-transfer transactions
are recorded in `docs/devnet-addresses.md`. The secret authority bytes must
never be copied into that registry.

The on-chain `TestFaucet` PDA remains available as a separate research path.
It must not be initialized for mints already controlled by the server faucet.

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
