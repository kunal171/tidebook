CREATE TABLE chain_transactions (
    signature TEXT PRIMARY KEY,
    slot BIGINT NOT NULL CHECK (slot >= 0),
    block_time BIGINT,
    raw_logs JSONB NOT NULL,
    schema_version INTEGER NOT NULL DEFAULT 1,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE chain_events (
    signature TEXT NOT NULL
        REFERENCES chain_transactions(signature)
        ON DELETE CASCADE,
    log_index INTEGER NOT NULL CHECK (log_index >= 0),
    event_type TEXT NOT NULL,
    payload_base64 TEXT NOT NULL,
    decoded_event JSONB NOT NULL,
    schema_version INTEGER NOT NULL DEFAULT 1,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (signature, log_index)
);

CREATE INDEX chain_transactions_slot_idx
    ON chain_transactions (slot);

CREATE INDEX chain_events_type_idx
    ON chain_events (event_type);
