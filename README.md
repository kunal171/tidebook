# Tidebook

Tidebook is a research-driven on-chain central limit order book for Solana,
built incrementally with Anchor, LiteSVM, and a companion web application.

## Current capabilities

- Create a deterministic market PDA for two distinct SPL Token mints.
- Validate base and quote mint accounts during market initialization.
- Pause, unpause, and close markets under authority control.
- Create deterministic bid and ask limit-order PDAs.
- Reject zero-price, zero-quantity, and paused-market orders.
- Exercise program behavior through LiteSVM integration tests.

Token custody, cancellation, price-level queues, matching, and settlement are
planned milestones. Current orders record intent but do not lock assets.

## Architecture

Detailed account models, instruction flows, invariants, limitations, and the
editable Draw.io diagram are maintained in [`docs/`](docs/README.md).

The two principal PDA schemes are:

```text
Market = ["market", base_mint, quote_mint]
Order  = ["order", market, order_id.to_le_bytes()]
```

## Repository layout

```text
tidebook/
├── app/                         # Companion web application
├── docs/                        # Architecture and research documentation
├── programs/tidebook/          # Anchor program and LiteSVM tests
├── Anchor.toml
└── rust-toolchain.toml
```

## Toolchain

- Rust `1.97.1`
- Anchor CLI `1.2.0`
- Solana CLI `4.1.2`
- LiteSVM `0.16.0`

## Build and test

```bash
anchor build
anchor test
```

`anchor test` runs the Rust LiteSVM suite configured in `Anchor.toml`; it does
not require a local validator.

## Program identity

```text
Honq7kkNfptR6XF5H4zn2jWqSmNRsteCpGwB8iG393cR
```

The same address is declared in the program, configured in `Anchor.toml`, and
derived from `target/deploy/tidebook-keypair.json`.

## Development approach

Tidebook is built in small feature branches. Each backend milestone is covered
by LiteSVM tests, exposed through the web application where appropriate, and
followed by an architecture-documentation update.
