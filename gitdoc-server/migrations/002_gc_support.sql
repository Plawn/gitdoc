-- Change symbols.parent_id FK to ON DELETE SET NULL so deleting orphan symbols
-- doesn't violate the self-referencing foreign key.
ALTER TABLE symbols DROP CONSTRAINT IF EXISTS symbols_parent_id_fkey;
ALTER TABLE symbols ADD CONSTRAINT symbols_parent_id_fkey
    FOREIGN KEY (parent_id) REFERENCES symbols(id) ON DELETE SET NULL;
