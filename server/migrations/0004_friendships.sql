CREATE TYPE friendship_status AS ENUM ('pending', 'accepted');

CREATE TABLE friendships (
    user_id    UUID              NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    friend_id  UUID              NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status     friendship_status NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ       NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, friend_id),
    CHECK (user_id <> friend_id)
);

CREATE INDEX friendships_friend_id_idx ON friendships(friend_id);
