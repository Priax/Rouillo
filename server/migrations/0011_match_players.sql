-- A match has any number of players: each one's slot, team, final place and
-- result are kept in match_stats, which already held a row per player.
ALTER TABLE match_stats DROP CONSTRAINT match_stats_slot_check;
ALTER TABLE match_stats ADD CONSTRAINT match_stats_slot_check CHECK (slot >= 1);
ALTER TABLE match_stats ADD COLUMN team SMALLINT NOT NULL DEFAULT 0;
ALTER TABLE match_stats ADD COLUMN place SMALLINT NOT NULL DEFAULT 1 CHECK (place >= 1);
ALTER TABLE match_stats ADD COLUMN won BOOLEAN NOT NULL DEFAULT false;

UPDATE match_stats ms
SET team = ms.slot,
    won = COALESCE(m.winner_slot = ms.slot, false),
    place = CASE WHEN m.winner_slot IS NULL OR m.winner_slot = ms.slot THEN 1 ELSE 2 END
FROM matches m
WHERE m.id = ms.match_id;

ALTER TABLE matches DROP COLUMN player1_id;
ALTER TABLE matches DROP COLUMN player2_id;
ALTER TABLE matches DROP COLUMN winner_slot;

CREATE INDEX matches_played_at_idx ON matches (played_at DESC, id);
