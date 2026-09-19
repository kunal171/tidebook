# Devnet Deployment

## Current deployment

| Property | Value |
| --- | --- |
| Cluster | Solana devnet |
| Program ID | `Honq7kkNfptR6XF5H4zn2jWqSmNRsteCpGwB8iG393cR` |
| ProgramData address | `4419TJGH2EMxFhpfpz1CANZrvyxirCjRbjMEMJeAhzPt` |
| Upgrade authority | `BX6RJHGbi7msj7t1ECCX6T1ZvvetHDK6UkjzAhPfWngq` |
| Initial deployment slot | `500917935` |

[Open the program in Solana Explorer](https://explorer.solana.com/address/Honq7kkNfptR6XF5H4zn2jWqSmNRsteCpGwB8iG393cR?cluster=devnet).

## Deploying updates

Build and verify before every upgrade:

```bash
anchor build
anchor test
anchor program deploy --no-idl
```

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

