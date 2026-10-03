-- Usernames are unique whatever their case.

-- The two "jujube" accounts belong to one player: keep the newer one (its
-- name and password), the higher ELO, and everything the older one played.
DO $$
DECLARE
    old_id UUID;
    new_id UUID;
BEGIN
    IF (SELECT count(*) FROM users WHERE lower(username) = 'jujube') <> 2 THEN
        RETURN;
    END IF;
    SELECT id INTO old_id FROM users WHERE lower(username) = 'jujube' ORDER BY created_at, id LIMIT 1;
    SELECT id INTO new_id FROM users WHERE lower(username) = 'jujube' AND id <> old_id;

    UPDATE users SET elo = GREATEST(elo, (SELECT elo FROM users WHERE id = old_id)) WHERE id = new_id;

    UPDATE matches SET player1_id = new_id WHERE player1_id = old_id;
    UPDATE matches SET player2_id = new_id WHERE player2_id = old_id;
    UPDATE match_stats SET user_id = new_id WHERE user_id = old_id;
    UPDATE ranked_series SET winner_id = new_id WHERE winner_id = old_id;
    UPDATE ranked_series SET loser_id = new_id WHERE loser_id = old_id;
    DELETE FROM matches WHERE player1_id = new_id AND player2_id = new_id;
    DELETE FROM ranked_series WHERE winner_id = new_id AND loser_id = new_id;

    DELETE FROM friendships WHERE user_id IN (old_id, new_id) AND friend_id IN (old_id, new_id);
    UPDATE friendships n SET status = 'accepted'
    WHERE (n.user_id = new_id OR n.friend_id = new_id)
      AND EXISTS (
          SELECT 1 FROM friendships o
          WHERE o.status = 'accepted'
            AND (o.user_id = old_id OR o.friend_id = old_id)
            AND (CASE WHEN o.user_id = old_id THEN o.friend_id ELSE o.user_id END)
              = (CASE WHEN n.user_id = new_id THEN n.friend_id ELSE n.user_id END)
      );
    DELETE FROM friendships o
    WHERE (o.user_id = old_id OR o.friend_id = old_id)
      AND EXISTS (
          SELECT 1 FROM friendships n
          WHERE (n.user_id = new_id OR n.friend_id = new_id)
            AND (CASE WHEN o.user_id = old_id THEN o.friend_id ELSE o.user_id END)
              = (CASE WHEN n.user_id = new_id THEN n.friend_id ELSE n.user_id END)
      );
    UPDATE friendships SET user_id = new_id WHERE user_id = old_id;
    UPDATE friendships SET friend_id = new_id WHERE friend_id = old_id;

    DELETE FROM users WHERE id = old_id;
END $$;

-- Any other duplicate: the oldest account keeps its name.
UPDATE users u
SET username = u.username || '_' || d.n
FROM (
    SELECT id, row_number() OVER (PARTITION BY lower(username) ORDER BY created_at, id) - 1 AS n
    FROM users
) d
WHERE u.id = d.id AND d.n > 0;

CREATE UNIQUE INDEX users_username_lower_idx ON users (lower(username));
