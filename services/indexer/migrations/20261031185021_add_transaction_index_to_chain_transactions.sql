ALTER TABLE chain_transactions
ADD COLUMN transaction_index INTEGER
CHECK (
    transaction_index IS NULL
    OR transaction_index >= 0
);

CREATE INDEX chain_transactions_position_idx
ON chain_transactions (
    slot DESC,
    transaction_index DESC NULLS LAST
);