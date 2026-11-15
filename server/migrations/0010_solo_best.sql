-- The best endless solo score of each account, kept across its devices.
ALTER TABLE users ADD COLUMN solo_best INTEGER NOT NULL DEFAULT 0 CHECK (solo_best >= 0);
