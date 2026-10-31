# Tidebook Indexer

Rust service that reads finalized Tidebook transactions, decodes all 18 Anchor
event types, and stores replayable source records in PostgreSQL. It is a
separate Cargo workspace so its RPC and database dependencies can evolve
independently from the on-chain program.

## Current scope

- Paginates finalized program history in bounded batches and resumes from a
  durable PostgreSQL cursor.
- Processes successful records oldest-first within each RPC page while using
  the raw page boundary, including failed entries, as the next cursor.
- Accepts `Program data:` only while Tidebook is the active invocation.
- Preserves raw logs, Base64 payloads, log indexes, decoded JSON, and schema
  versions.
- Writes each transaction and its events atomically.
- Treats exact replay as success and rejects conflicting source data.
- Advances the checkpoint only after every successful record in a page commits.

The service currently performs historical backfill. Live subscriptions,
read-optimized projections, reconciliation, and APIs remain separate phases.

## Runtime structure

```text
main -> backfill -> processor -> reporter
          |            |
          |            `-> strict event decoder -> source storage
          `-> Solana RPC pages -> durable checkpoint
```

`main` only constructs configuration, RPC, and database dependencies.
`backfill` owns bounded pagination and checkpoint order. `processor` fetches,
decodes, and persists one transaction. `reporter` keeps human-readable event
output outside ingestion control flow.

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

Run the isolated PostgreSQL integration tests with:

```bash
DATABASE_URL=postgres://tidebook:tidebook_dev@127.0.0.1:5433/postgres \
  cargo test --manifest-path services/indexer/Cargo.toml -- --ignored
```

## Safety guarantees

- Failed or non-finalized transactions are never persisted.
- Unknown, malformed, truncated, and trailing event data fails closed.
- Nested CPI data cannot be misclassified as a Tidebook event.
- Integer conversions are checked before opening a database transaction.
- Conflicts roll back the complete transaction and event batch.
- A transaction, decoder, or storage error leaves the page checkpoint unchanged.
- A terminal empty page retains the last historical cursor for auditing.
- Each run stops after a configured page cap and can resume safely.

## Next

1. WebSocket live sync with a separate newest-signature high-water mark and
   checkpoint-based gap recovery.
2. Read-optimized projections and an API.
3. Account reconciliation and operational monitoring.
