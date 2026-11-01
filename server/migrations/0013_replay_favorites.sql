-- The replays each player chose to keep. A replay nobody keeps goes after a day.
CREATE TABLE replay_favorites (
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    match_id UUID NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, match_id)
);

CREATE INDEX replay_favorites_match_idx ON replay_favorites (match_id);
CREATE INDEX matches_replay_idx ON matches (played_at) WHERE replay IS NOT NULL;
