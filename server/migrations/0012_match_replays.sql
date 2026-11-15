-- What a match needs to be played again: its seed, rules and every board's
-- inputs and nuisance, encoded by the shared crate. Older matches have none.
ALTER TABLE matches ADD COLUMN replay BYTEA;
