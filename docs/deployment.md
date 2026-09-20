# Devnet Deployment

## Current deployment

| Property | Value |
| --- | --- |
| Cluster | Solana devnet |
| Program ID | `BPdNF5CnV8z1EkHo7tcueR6wXmzZV2j6j4wsUTirPgWL` |
| ProgramData address | `HvXrEzK1NeTQg3PhRsVFXPyshAe6mj4L5PwK132xn26g` |
| Upgrade authority | `BX6RJHGbi7msj7t1ECCX6T1ZvvetHDK6UkjzAhPfWngq` |
| Initial deployment slot | `501476192` |

[Open the program in Solana Explorer](https://explorer.solana.com/address/BPdNF5CnV8z1EkHo7tcueR6wXmzZV2j6j4wsUTirPgWL?cluster=devnet).

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
