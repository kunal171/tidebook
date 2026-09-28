# Tidebook Documentation

This directory describes the design of the Solana limit order book as it exists
today. It also records the boundary between the implemented foundation and the
remaining research milestones that are still planned.

## Documents

- [Architecture](architecture.md): program boundaries, accounts, instructions,
  invariants, transaction flows, tests, and planned evolution.
- [Price-level and FIFO design](price-level-fifo-design.md): proposed account
  layout, mutation rules, security invariants, tradeoffs, and test matrix for
  the order-book index milestone.
- [Crankless matching and settlement](crankless-settlement-design.md): Phoenix
  and OpenBook comparison, Tidebook's bounded PDA-based settlement model,
  trader balances, matching account contract, and implementation sequence.
- [Future enhancements](future-enhancements.md): staged oracle, reference
  market-maker, monitoring, standalone AMM, hybrid routing, and benchmarking
  roadmap with safety gates and effort estimates.
- [Devnet deployment](deployment.md): stable program identity, upgrade details,
  verification, and the current IDL-upload limitation.
- [Devnet address registry](devnet-addresses.md): program, governance, mint,
  market, and transaction addresses used by the shared research deployment.
- [Draw.io architecture diagram](diagrams/tidebook-architecture.drawio):
  editable source for the current and planned architecture.

Open the `.drawio` file with [draw.io](https://app.diagrams.net/) or a compatible
editor. Blue and green elements are implemented. Gray dashed elements are
planned.

## Current verification

From the project root:

```bash
anchor build
anchor test
```

The Anchor test script runs the Rust LiteSVM integration-test suite.
