# Solana Limit Order Book

A Solana on-chain limit order book being built step by step with Anchor and Rust.

The project currently implements the first foundation: creating a market account for a base/quote asset pair. Order accounts, order placement, matching, and settlement are planned next.

## Current milestone

Implemented instruction:

- `initialize_market`

The instruction creates a `Market` PDA using these seeds:

```text
["market", base_mint_pubkey, quote_mint_pubkey]
```

Each base/quote pair therefore gets its own deterministic market account.

The current `Market` account stores:

- market authority
- base mint public key
- quote mint public key
- market status
- next order ID
- best bid
- best ask
- PDA bump

## Project layout

```text
solana_lob/
├── Anchor.toml
├── programs/solana_lob/src/
│   ├── constants.rs
│   ├── error.rs
│   ├── instructions/
│   │   └── initialize.rs
│   ├── state.rs
│   └── lib.rs
└── rust-toolchain.toml
```

## Requirements

- Rust `1.89.0`
- Anchor CLI `1.2.0`
- Solana CLI compatible with the configured Anchor toolchain
- A configured Solana wallet at `~/.config/solana/id.json` for local deployment and tests

## Build

From this directory:

```bash
anchor build
```

Run the Rust test suite:

```bash
cargo test
```

The local validator is currently skipped in `Anchor.toml`, so `anchor build` is the primary validation command for the current milestone.

## Program ID

```text
E5Ms8cNg6Xvy7RLwWVgimRZwZkhcXNjGMRZRnon5Tt1D
```

The same ID is configured in `Anchor.toml` and declared in `src/lib.rs`.

## Design notes

A market represents a trading pair, not a single asset. For example:

```text
base mint:  SOL
quote mint: USDC
```

and

```text
base mint:  BTC
quote mint: USDC
```

produce different market PDAs.

The initializer currently accepts the two mint accounts as `UncheckedAccount` values because it only needs their public keys for PDA derivation. SPL mint validation should be added before production use and before funds or orders are handled.

## Roadmap

1. Add validation for base and quote mint accounts.
2. Add the `Order` account and order-side/type enums.
3. Implement `place_limit_order`.
4. Add price levels and order queues.
5. Implement matching and settlement.
6. Add LiteSVM integration tests for market creation and order behavior.
