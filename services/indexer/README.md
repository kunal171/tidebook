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
- strictly Borsh-decodes all 18 currently emitted governance, market, balance,
  order, fill, fee, and faucet events;
- connects to PostgreSQL and applies embedded migrations before ingestion;
- atomically stores each finalized transaction with all its decoded events;
- accepts exact source-record replays without creating duplicates; and
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
DATABASE_URL=postgres://tidebook:tidebook_dev@127.0.0.1:5433/tidebook_indexer
```

The current configuration loader reads process environment variables directly;
it does not load `.env` files automatically. Export the file before running:

```bash
set -a
source services/indexer/.env
set +a
```

`DATABASE_URL` is required. The indexer exits before RPC ingestion when the
database is unavailable or its migrations cannot be applied.

## Local PostgreSQL

The development database binds to `127.0.0.1:5433` because this workstation's
host PostgreSQL service already occupies port `5432`.

Start PostgreSQL and wait for its health check:

```bash
docker compose -f services/indexer/compose.yaml up -d
docker compose -f services/indexer/compose.yaml ps
```

Apply or inspect migrations manually when needed:

```bash
cd services/indexer
sqlx migrate info
sqlx migrate run
```

Stop PostgreSQL without deleting its named data volume:

```bash
docker compose -f services/indexer/compose.yaml stop
```

## Run

From the repository root:

```bash
cargo run --manifest-path services/indexer/Cargo.toml
```

The current executable inspects at most 20 recent finalized transactions,
prints the raw Base64 event payload with its original transaction-log index,
and prints typed fields for every currently emitted Tidebook event. It is a
diagnostic ingestion batch, not yet a continuous indexer.

## Verify

```bash
cargo fmt --manifest-path services/indexer/Cargo.toml --all --check
cargo test --manifest-path services/indexer/Cargo.toml
cargo clippy --manifest-path services/indexer/Cargo.toml --all-targets -- -D warnings
```

The PostgreSQL integration test is ignored by the default suite so contributors
without a database can still run unit tests. Run it against an isolated SQLx
test database with:

```bash
DATABASE_URL=postgres://tidebook:tidebook_dev@127.0.0.1:5433/postgres \
  cargo test --manifest-path services/indexer/Cargo.toml \
  storage::writer::tests::exact_replay_is_idempotent_and_conflicts_roll_back \
  -- --ignored --exact
```

## Guarantees and limitations

- Only successful finalized transactions become source records.
- Missing log fields normalize to an empty log collection.
- Transport and parsing failures remain errors so a future checkpoint must not
  advance past data that was never processed.
- Raw event order is preserved using the original `log_index`.
- All 18 event schemas in the current Anchor IDL are decoded. Unknown future
  discriminators and malformed known payloads deliberately stop the batch.
- A source transaction and all its decoded events commit atomically.
- Exact replay is idempotent; conflicting source data for an existing identity
  is rejected and rolled back.
- Raw logs, Base64 event payloads, decoded JSON, and schema versions are kept
  together for future replay and schema evolution.
- Signature pagination, durable checkpoints, WebSocket subscriptions,
  projection tables, and APIs are not implemented yet.

## Follow-up milestones

1. `feature/indexer-backfill`: pagination and durable finalized checkpoints.
2. `feature/indexer-live-sync`: WebSocket ingestion with checkpoint-based gap
   recovery.
3. Projection tables and an API for frontend and monitoring queries.
