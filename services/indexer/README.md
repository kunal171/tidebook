# Tidebook Indexer

The Tidebook indexer is an off-chain Rust service for turning finalized Solana
transactions into reliable, replayable source records. It is intentionally a
separate Cargo workspace so its RPC dependencies can evolve independently from
the Anchor and LiteSVM versions used by the on-chain program.

## Current scope

The service currently:

- reads its RPC URL and Tidebook program address from the environment;
- requests the latest successful finalized signatures involving Tidebook;
- fetches full transaction metadata and execution logs;
- rejects failed transactions because their state changes were rolled back;
- preserves raw logs in their original order;
- tracks nested Solana program invocations; and
- extracts `Program data:` payloads only while Tidebook is the active program;
- preserves each raw Base64 payload and original transaction-log index;
- validates Anchor's eight-byte event discriminator;
- strictly Borsh-decodes market and order lifecycle events; and
- stops on malformed or unsupported events instead of silently producing a
  partial projection.

Program scoping matters because a Tidebook instruction can invoke other
programs through CPI. A nested program's data log must never be decoded as a
Tidebook event.

## Configuration

Copy the example environment file or export the variables in your shell:

```bash
cp services/indexer/.env.example services/indexer/.env
```

```text
SOLANA_RPC_URL=https://api.devnet.solana.com
TIDEBOOK_PROGRAM_ID=AG4ztZvcZjzFKjoXLzH8M6U3if9CwVFTXH2FHjqg7SdL
```

The current configuration loader reads process environment variables directly;
it does not load `.env` files automatically. Export the file before running:

```bash
set -a
source services/indexer/.env
set +a
```

## Run

From the repository root:

```bash
cargo run --manifest-path services/indexer/Cargo.toml
```

The current executable inspects at most 20 recent finalized transactions,
prints the raw Base64 event payload with its original transaction-log index,
and prints typed fields for supported market and order events. It is a
diagnostic ingestion batch, not yet a continuous indexer.

## Verify

```bash
cargo fmt --manifest-path services/indexer/Cargo.toml --all --check
cargo test --manifest-path services/indexer/Cargo.toml
cargo clippy --manifest-path services/indexer/Cargo.toml --all-targets -- -D warnings
```

## Guarantees and limitations

- Only successful finalized transactions become source records.
- Missing log fields normalize to an empty log collection.
- Transport and parsing failures remain errors so a future checkpoint must not
  advance past data that was never processed.
- Raw event order is preserved using the original `log_index`.
- Market initialization, market status, market closure, order placement, and
  order cancellation are decoded. The other 13 emitted event types remain
  unsupported and deliberately stop the batch as unknown discriminators.
- Signature pagination, durable checkpoints, PostgreSQL persistence, replay,
  WebSocket subscriptions, projections, and APIs are not implemented yet.

## Follow-up milestones

1. `feature/indexer-event-catalog`: add the remaining admin, balance, faucet,
   fee, and fill event families to the deterministic decoder.
2. `feature/indexer-storage`: append-only PostgreSQL source tables and
   idempotent event writes.
3. `feature/indexer-backfill`: pagination and durable finalized checkpoints.
4. `feature/indexer-live-sync`: WebSocket ingestion with checkpoint-based gap
   recovery.
