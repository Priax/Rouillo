ALTER TABLE matches ADD COLUMN ranked BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE matches ALTER COLUMN winner_slot DROP NOT NULL;

CREATE TABLE ranked_series (
    id        UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    played_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    winner_id UUID        REFERENCES users(id) ON DELETE SET NULL,
    loser_id  UUID        REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX ranked_series_winner_idx ON ranked_series(winner_id);
CREATE INDEX ranked_series_loser_idx ON ranked_series(loser_id);
