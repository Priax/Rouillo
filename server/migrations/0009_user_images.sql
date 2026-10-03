CREATE TABLE user_images (
    user_id UUID    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind    TEXT    NOT NULL CHECK (kind IN ('avatar', 'banner')),
    version INTEGER NOT NULL DEFAULT 1,
    data    BYTEA   NOT NULL,
    PRIMARY KEY (user_id, kind)
);
