//! The HTTP API's bodies, as the server writes them and the client reads them, both ways.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
pub use uuid::Uuid;

/// What `register` and `login` take.
#[derive(Serialize, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// What `me/password` takes.
#[derive(Serialize, Deserialize)]
pub struct NewPassword {
    pub current: String,
    pub new: String,
}

/// What `me/username` takes: the new name, and the password that allows it.
#[derive(Serialize, Deserialize)]
pub struct Rename {
    pub username: String,
    pub password: String,
}

/// What `me/delete` takes.
#[derive(Serialize, Deserialize)]
pub struct Password {
    pub password: String,
}

/// What `PATCH me` takes: the fields to change.
#[derive(Serialize, Deserialize)]
pub struct AccountPatch {
    pub bio: Option<String>,
    pub favorite_music: Option<String>,
}

/// What `me/solo-best` takes.
#[derive(Serialize, Deserialize)]
pub struct Score {
    pub score: i32,
}

/// What `POST friends` takes: whom to ask.
#[derive(Serialize, Deserialize)]
pub struct FriendRequest {
    pub user_id: Uuid,
}

/// A session, from `register` and `login`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Auth {
    pub token: String,
    pub user_id: Uuid,
    pub username: String,
    pub elo: i32,
    pub avatar_url: Option<String>,
}

/// The signed-in account, from `me` and from what changes it.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Account {
    pub id: Uuid,
    pub username: String,
    pub bio: Option<String>,
    pub favorite_music: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub elo: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub solo_best: i32,
}

/// A player's profile and stats, from `users/{id}`.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Profile {
    pub id: Uuid,
    pub username: String,
    pub bio: Option<String>,
    pub favorite_music: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub elo: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub total_matches: i64,
    pub wins: i64,
    pub casual_matches: i64,
    pub casual_wins: i64,
    pub ranked_series: i64,
    pub ranked_series_won: i64,
    pub all_time_max_chain: i32,
    pub total_nuisance_sent: i64,
    pub total_all_clears: i64,
}

/// Nothing known yet, before the profile comes.
impl Default for Profile {
    fn default() -> Self {
        Self {
            id: Uuid::nil(),
            username: String::new(),
            bio: None,
            favorite_music: None,
            avatar_url: None,
            banner_url: None,
            elo: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            total_matches: 0,
            wins: 0,
            casual_matches: 0,
            casual_wins: 0,
            ranked_series: 0,
            ranked_series_won: 0,
            all_time_max_chain: 0,
            total_nuisance_sent: 0,
            total_all_clears: 0,
        }
    }
}

/// A player in a list: a friend, a request, a search result.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Friend {
    pub user_id: Uuid,
    pub username: String,
    pub elo: i32,
    pub avatar_url: Option<String>,
    /// Set from the game server, not the database: whether they are in a room.
    #[cfg_attr(feature = "sqlx", sqlx(default))]
    pub playing: bool,
}

/// From `GET friends`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FriendList {
    pub friends: Vec<Friend>,
    pub sent: Vec<Friend>,
    pub received: Vec<Friend>,
}

/// A line of `leaderboard`.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct LeaderboardEntry {
    pub rank: i64,
    pub user_id: Uuid,
    pub username: String,
    pub elo: i32,
    pub series: i64,
    pub series_won: i64,
    pub avatar_url: Option<String>,
}

/// A match of `users/{id}/matches`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MatchEntry {
    pub id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub played_at: OffsetDateTime,
    pub duration_secs: f64,
    pub ranked: bool,
    pub has_replay: bool,
    pub favorite: bool,
    pub players: Vec<MatchPlayer>,
}

impl MatchEntry {
    pub fn me(&self, user: Uuid) -> Option<&MatchPlayer> {
        self.players.iter().find(|p| p.user_id == Some(user))
    }

    /// The other player of a one against one.
    pub fn opponent(&self, user: Uuid) -> Option<&MatchPlayer> {
        let [a, b] = self.players.as_slice() else {
            return None;
        };
        Some(if a.user_id == Some(user) { b } else { a })
    }

    pub fn drawn(&self) -> bool {
        self.players.iter().all(|p| !p.won)
    }
}

/// A player of a [`MatchEntry`]: without an account if theirs was deleted.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct MatchPlayer {
    pub user_id: Option<Uuid>,
    pub username: Option<String>,
    pub slot: i16,
    pub team: i16,
    pub place: i16,
    pub won: bool,
    pub max_chain: i16,
    pub total_chains: i16,
    pub nuisance_sent: i32,
    pub nuisance_received: i32,
    pub all_clears: i16,
    pub pieces_placed: i32,
}

/// From `me/solo-best`: the account's best, which may be another device's.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct SoloBest {
    pub solo_best: i32,
}

/// From `transport`, when the server takes WebTransport sessions.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Offer {
    pub url: String,
    /// The SHA-256 of a self-signed certificate, which a browser accepts only when told this hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_hash: Option<Vec<u8>>,
}

/// From `version`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Version {
    pub version: String,
}

/// What every refused request answers: a code the client translates, and the server's own words.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Refusal {
    pub error: String,
    pub code: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "00000000-0000-0000-0000-000000000001";
    const ENTRY: &str = r#"{"id":"00000000-0000-0000-0000-0000000000aa","played_at":"2026-11-27T12:00:00Z",
        "duration_secs":12.5,"ranked":false,"has_replay":true,"favorite":false,"players":[
        {"user_id":"00000000-0000-0000-0000-000000000001","username":"moi","slot":1,"team":1,"place":2,"won":false,
         "max_chain":3,"total_chains":1,"nuisance_sent":10,"nuisance_received":4,"all_clears":0,"pieces_placed":9},
        {"user_id":null,"username":null,"slot":2,"team":2,"place":1,"won":true,"max_chain":1,"total_chains":1,
         "nuisance_sent":4,"nuisance_received":10,"all_clears":0,"pieces_placed":8}]}"#;

    fn me() -> Uuid {
        ME.parse().expect("a uuid")
    }

    #[test]
    fn a_match_reads_with_its_players() {
        let m: MatchEntry = serde_json::from_str(ENTRY).expect("parses");
        assert_eq!(m.me(me()).map(|p| (p.place, p.won, p.max_chain)), Some((2, false, 3)));
        assert_eq!(m.opponent(me()).map(|p| p.won), Some(true));
        assert!(!m.drawn());
    }

    #[test]
    fn a_game_of_four_has_no_single_opponent() {
        let mut m: MatchEntry = serde_json::from_str(ENTRY).expect("parses");
        let more: MatchEntry = serde_json::from_str(ENTRY).expect("parses");
        m.players.extend(more.players);
        assert!(m.opponent(me()).is_none());
        assert!(m.me(me()).is_some());
    }

    #[test]
    fn a_match_goes_back_to_what_it_was_written_from() {
        let m: MatchEntry = serde_json::from_str(ENTRY).expect("parses");
        let again: MatchEntry = serde_json::from_str(&serde_json::to_string(&m).expect("writes")).expect("reads");
        assert_eq!((again.id, again.played_at, again.players.len()), (m.id, m.played_at, 2));
    }
}
