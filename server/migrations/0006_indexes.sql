-- Prevent two simultaneous friend requests between the same pair of users
-- (e.g. (A→B) + (B→A) coexisting). LEAST/GREATEST canonicalises the pair
-- so the unique index covers both orderings.
CREATE UNIQUE INDEX friendships_canonical_idx
    ON friendships (LEAST(user_id, friend_id), GREATEST(user_id, friend_id));

-- Speeds up the hourly DELETE FROM sessions WHERE expires_at < now()
CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);
