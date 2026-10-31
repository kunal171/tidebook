# Tidebook Indexer

Rust service that reads finalized Tidebook transactions, decodes all 18 Anchor
event types, and stores replayable source records in PostgreSQL. It is a
separate Cargo workspace so its RPC and database dependencies can evolve
independently from the on-chain program.

## Current scope

- Fetches the latest 20 successful finalized transactions from Solana RPC.
- Accepts `Program data:` only while Tidebook is the active invocation.
- Preserves raw logs, Base64 payloads, log indexes, decoded JSON, and schema
  versions.
- Writes each transaction and its events atomically.
- Treats exact replay as success and rejects conflicting source data.

This is a diagnostic ingestion batch. Pagination, durable checkpoints, live
subscriptions, projections, and APIs are not implemented yet.

## Run locally

From the repository root:

```bash
cp services/indexer/.env.example services/indexer/.env
docker compose -f services/indexer/compose.yaml up -d

set -a
source services/indexer/.env
set +a

cargo run --manifest-path services/indexer/Cargo.toml
```

The local database uses `127.0.0.1:5433` because port `5432` is already used by
the host PostgreSQL service. `DATABASE_URL` is required, and pending migrations
run automatically before RPC ingestion.

Useful database commands:

```bash
cd services/indexer
sqlx migrate info
sqlx migrate run

docker compose -f compose.yaml stop
```

## Verify

```bash
cargo fmt --manifest-path services/indexer/Cargo.toml --all --check
cargo test --manifest-path services/indexer/Cargo.toml
cargo clippy --manifest-path services/indexer/Cargo.toml --all-targets -- -D warnings
```

Run the isolated PostgreSQL integration test with:

```bash
DATABASE_URL=postgres://tidebook:tidebook_dev@127.0.0.1:5433/postgres \
  cargo test --manifest-path services/indexer/Cargo.toml \
  storage::writer::tests::exact_replay_is_idempotent_and_conflicts_roll_back \
  -- --ignored --exact
```

## Safety guarantees

- Failed or non-finalized transactions are never persisted.
- Unknown, malformed, truncated, and trailing event data fails closed.
- Nested CPI data cannot be misclassified as a Tidebook event.
- Integer conversions are checked before opening a database transaction.
- Conflicts roll back the complete transaction and event batch.

## Next

1. Paginated finalized backfill with durable checkpoints.
2. WebSocket live sync with checkpoint-based gap recovery.
3. Read-optimized projections and an API.
