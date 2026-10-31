-- Tracks historical RPC pagination independently for each indexed program.
--
-- `before_signature` is the cursor passed to the next
-- getSignaturesForAddress request. It must only advance after every successful
-- transaction from the current page has been persisted.
CREATE TABLE backfill_checkpoints (
    program_id TEXT PRIMARY KEY,
    before_signature TEXT,
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE backfill_checkpoints IS
    'Durable progress for finalized Solana transaction backfills';

COMMENT ON COLUMN backfill_checkpoints.before_signature IS
    'Oldest raw RPC signature from the last completely persisted page';

COMMENT ON COLUMN backfill_checkpoints.completed IS
    'True after RPC returns no more historical signatures';
