# Tidebook Documentation

This directory describes the design of the Solana limit order book as it exists
today. It also records the boundary between the implemented foundation and the
exchange functionality that is still planned.

## Documents

- [Architecture](architecture.md): program boundaries, accounts, instructions,
  invariants, transaction flows, tests, and planned evolution.
- [Devnet deployment](deployment.md): stable program identity, upgrade details,
  verification, and the current IDL-upload limitation.
- [Devnet address registry](devnet-addresses.md): program, governance, mint,
  market, and transaction addresses used by the shared research deployment.
- [Draw.io architecture diagram](diagrams/tidebook-architecture.drawio):
  editable source for the current and planned architecture.

Open the `.drawio` file with [draw.io](https://app.diagrams.net/) or a compatible
editor. The blue elements are implemented. The gray dashed elements are planned.

## Current verification

From the project root:

```bash
anchor build
anchor test
```

The Anchor test script runs the Rust LiteSVM integration-test suite.
