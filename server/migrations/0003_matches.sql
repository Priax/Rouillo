CREATE TABLE matches (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    played_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    duration_secs FLOAT      NOT NULL,
    player1_id   UUID        REFERENCES users(id) ON DELETE SET NULL,
    player2_id   UUID        REFERENCES users(id) ON DELETE SET NULL,
    winner_slot  SMALLINT    NOT NULL CHECK (winner_slot IN (1, 2))
);

CREATE TABLE match_stats (
    match_id         UUID     NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    user_id          UUID     REFERENCES users(id) ON DELETE SET NULL,
    slot             SMALLINT NOT NULL CHECK (slot IN (1, 2)),
    max_chain        SMALLINT NOT NULL DEFAULT 0,
    total_chains     SMALLINT NOT NULL DEFAULT 0,
    nuisance_sent    INTEGER  NOT NULL DEFAULT 0,
    nuisance_received INTEGER NOT NULL DEFAULT 0,
    all_clears       SMALLINT NOT NULL DEFAULT 0,
    pieces_placed    INTEGER  NOT NULL DEFAULT 0,
    PRIMARY KEY (match_id, slot)
);

CREATE INDEX match_stats_user_id_idx ON match_stats(user_id);
CREATE INDEX matches_player1_idx ON matches(player1_id);
CREATE INDEX matches_player2_idx ON matches(player2_id);
