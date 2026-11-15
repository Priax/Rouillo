CREATE TABLE users (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    username      TEXT        NOT NULL UNIQUE,
    password_hash TEXT        NOT NULL,
    bio           TEXT,
    favorite_music TEXT,
    avatar_url    TEXT,
    banner_url    TEXT,
    elo           INTEGER     NOT NULL DEFAULT 1000,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
