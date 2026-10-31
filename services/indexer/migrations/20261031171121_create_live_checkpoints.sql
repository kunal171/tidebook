-- Tracks the newest finalized transaction completely persisted by live sync.
--
-- This cursor moves forward and is intentionally separate from the historical
-- backfill cursor, which moves backward through older pages.
-- `transaction_index` is nullable because not every RPC source supplies it;
-- code must fail closed for ambiguous same-slot advancement when it is absent.
CREATE TABLE live_checkpoints (
    program_id TEXT PRIMARY KEY,
    latest_finalized_signature TEXT NOT NULL,
    latest_finalized_slot BIGINT NOT NULL
        CHECK (latest_finalized_slot >= 0),
    transaction_index INTEGER
        CHECK (transaction_index IS NULL OR transaction_index >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE live_checkpoints IS
    'Newest completely persisted finalized position for forward live ingestion';

COMMENT ON COLUMN live_checkpoints.latest_finalized_signature IS
    'Signature used as the HTTP catch-up until cursor after reconnect';

COMMENT ON COLUMN live_checkpoints.latest_finalized_slot IS
    'Finalized slot retained for monotonicity checks and operational visibility';

COMMENT ON COLUMN live_checkpoints.transaction_index IS
    'Optional execution position within the slot for deterministic same-slot ordering';
