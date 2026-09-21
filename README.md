# Tidebook

Tidebook is a research-driven on-chain central limit order book for Solana,
built incrementally with Anchor, LiteSVM, and a companion web application.

## Current capabilities

- Create a deterministic market PDA for two distinct SPL Token mints.
- Validate base and quote mint accounts during market initialization.
- Pause, unpause, and close markets under authority control.
- Create deterministic bid and ask limit-order PDAs.
- Enforce fixed-point price ticks, quantity lots, and nonzero quote notional.
- Lock quote collateral for bids and base collateral for asks in canonical vaults.
- Cancel open orders under owner control, including while a market is paused.
- Refund collateral on cancellation and safely close empty, paused markets.
- Exercise program behavior through LiteSVM integration tests.

Price-level queues, matching, and settlement are planned milestones. Current
orders lock assets but do not yet match or settle trades.

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
anchor test --skip-deploy
```

`anchor test --skip-deploy` runs the Rust LiteSVM suite configured in
`Anchor.toml`; it does not require a local validator and does not deploy to the
configured devnet cluster. Bare `anchor test` deploys before running the suite
and must not be used as the local test command.

Deploy or upgrade the configured program on devnet with:

```bash
anchor program deploy --no-idl
```

The temporary `--no-idl` flag bypasses an upstream metadata-CLI packaging issue;
the application consumes the locally generated IDL instead.

## Program identity

```text
BPdNF5CnV8z1EkHo7tcueR6wXmzZV2j6j4wsUTirPgWL
```

The same address is declared in the program, configured in `Anchor.toml`, and
derived from `target/deploy/tidebook-keypair.json`.

The program is live on
[Solana devnet](https://explorer.solana.com/address/BPdNF5CnV8z1EkHo7tcueR6wXmzZV2j6j4wsUTirPgWL?cluster=devnet).

## Development approach

Tidebook is built in small feature branches. Each backend milestone is covered
by LiteSVM tests, exposed through the web application where appropriate, and
followed by an architecture-documentation update.
