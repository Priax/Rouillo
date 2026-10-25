use std::collections::HashMap;
use std::ops::Range;

use bytes::Bytes;
use shared::{LobbyInfo, RankedInfo, RoomId, RoomInfo, RoomSettings, ServerMessage};
use tokio::time::Instant;
use tracing::info;
use uuid::Uuid;

use crate::sim::{slot_byte, Sim};
use crate::{db, ConnId, Token};

pub enum Phase {
    Lobby,
    CountingDown(f32),
    Playing,
}

pub struct Member {
    pub token: Token,
    pub conn: Option<ConnId>,
    pub disconnect_at: Option<Instant>,
    pub user_id: Option<Uuid>,
    pub name: String,
    /// Picked in the lobby; with none, the player plays for themselves.
    pub team: Option<u8>,
    /// The board this member plays in the running game: none for one who
    /// came in after it started.
    pub game_slot: Option<usize>,
}

impl Member {
    pub const fn present(token: Token, conn: ConnId, user_id: Option<Uuid>, name: String) -> Self {
        Self {
            token,
            conn: Some(conn),
            disconnect_at: None,
            user_id,
            name,
            team: None,
            game_slot: None,
        }
    }
}

pub struct Spectator {
    pub conn: ConnId,
    pub user_id: Option<Uuid>,
    pub name: String,
}

pub struct Series {
    pub users: [Uuid; 2],
    pub names: [String; 2],
    pub elos: [i32; 2],
    pub avatars: [Option<String>; 2],
    pub wins: [u8; 2],
    pub next_game_at: Option<Instant>,
    pub result: Option<SeriesResult>,
}

#[derive(Clone, Copy)]
pub struct SeriesResult {
    pub winner: Option<usize>,
    pub elo_changes: [i32; 2],
}

pub struct Room {
    pub id: RoomId,
    pub name: String,
    pub host: Token,
    pub members: Vec<Member>,
    pub settings: RoomSettings,
    pub phase: Phase,
    pub sim: Sim,
    pub series: Option<Series>,
    pub spectators: Vec<Spectator>,
    /// The board watched large by those without one of their own, by game
    /// slot: the first unless they picked another this game.
    pub follows: HashMap<ConnId, usize>,
    /// Who sits in each game slot, as the game started.
    pub seats: Vec<Seat>,
}

pub struct Seat {
    pub user_id: Option<Uuid>,
    pub name: String,
}

/// From this many boards, the others than the one watched large are sent as
/// thumbnails, one tick in `OTHERS_EVERY`, and smoothed between updates.
const SPARSE_FROM: usize = 3;
const OTHERS_EVERY: u32 = 6;

/// One tick's game state for one connection.
pub enum StateOut {
    /// In one message, on a link that keeps every message.
    Whole(Vec<u8>),
    /// Messages of a few boards, each to go alone in a datagram.
    Boards(Vec<Bytes>),
}

/// What a packed state may weigh: under the datagram size of a path with
/// the smallest MTU QUIC allows, framing included.
pub const DATAGRAM_BUDGET: usize = 1100;

/// The boards in `slots`, as `encode` makes them, in as few datagrams as fit
/// `DATAGRAM_BUDGET`: boards compressed together cost a fraction of each
/// alone. A board too big on its own still goes, and takes the stream.
fn packed(slots: Range<usize>, encode: &impl Fn(Range<usize>) -> Vec<u8>, out: &mut Vec<Bytes>) {
    let bytes = encode(slots.clone());
    if bytes.len() <= DATAGRAM_BUDGET || slots.len() == 1 {
        out.push(Bytes::from(bytes));
        return;
    }
    let mid = slots.start + slots.len() / 2;
    packed(slots.start..mid, encode, out);
    packed(mid..slots.end, encode, out);
}

impl Room {
    pub fn new(id: RoomId, name: String, members: Vec<Member>, settings: RoomSettings, series: Option<Series>) -> Self {
        let members_len = members.len().max(2);
        Self {
            id,
            name,
            host: members[0].token.clone(),
            members,
            settings,
            phase: Phase::Lobby,
            sim: Sim::new(&settings, (0..members_len).map(slot_byte).collect()),
            series,
            spectators: Vec::new(),
            follows: HashMap::new(),
            seats: Vec::new(),
        }
    }

    /// Seats every member for a new game, each on their team.
    pub fn start_game(&mut self) {
        for (i, m) in self.members.iter_mut().enumerate() {
            m.game_slot = Some(i);
        }
        self.follows.clear();
        self.seats = self
            .members
            .iter()
            .map(|m| Seat {
                user_id: m.user_id,
                name: m.name.clone(),
            })
            .collect();
        let teams = self.teams();
        let game = self.sim.game.wrapping_add(1);
        self.sim.reset_boards(&self.settings, teams);
        self.sim.game = game;
    }

    pub fn game_slot_of_conn(&self, conn: ConnId) -> Option<usize> {
        let slot = self.members.iter().find(|m| m.conn == Some(conn))?.game_slot?;
        (slot < self.sim.boards.len()).then_some(slot)
    }

    /// Whether the members make at least two sides.
    pub fn has_sides(&self) -> bool {
        let teams = self.teams();
        teams.iter().any(|&t| t != teams[0])
    }

    pub fn back_to_lobby(&mut self) {
        self.phase = Phase::Lobby;
        self.sim.finished = false;
        self.sim.set_paused(false);
    }

    pub fn lobby_payloads(&self) -> Vec<(ConnId, Vec<u8>)> {
        let encode = |info: LobbyInfo| shared::encode(&ServerMessage::Lobby { info });
        let members = self
            .members
            .iter()
            .enumerate()
            .filter_map(|(i, m)| Some((m.conn?, encode(self.lobby_info_for(i)))));
        let watching = encode(self.spectator_info());
        let spectators = self.spectators.iter().map(|s| (s.conn, watching.clone()));
        members.chain(spectators).collect()
    }

    /// The start of the running game as `conn` sees it: its own slot, 0 for
    /// one without a board, and every seat's name. A client starts each
    /// game it is told of afresh, so its slot does too.
    pub fn game_start_for(&mut self, conn: ConnId) -> ServerMessage {
        let slot = self.game_slot_of_conn(conn);
        if let Some(slot) = slot {
            self.sim.restart_client(slot);
        }
        let your_slot = slot.map_or(0, |s| slot_byte(s + 1));
        ServerMessage::GameStart {
            game: self.sim.game,
            your_slot,
            names: self.seats.iter().map(|s| s.name.clone()).collect(),
        }
    }

    fn send_to_audience(&self, payload: &[u8], outgoing: &mut Vec<(ConnId, Vec<u8>)>) {
        for c in self.audience() {
            outgoing.push((c, payload.to_vec()));
        }
    }

    pub fn game_start_payloads(&mut self) -> Vec<(ConnId, Vec<u8>)> {
        self.audience()
            .into_iter()
            .map(|c| (c, shared::encode(&self.game_start_for(c))))
            .collect()
    }

    pub fn spectator_slot(&self, conn: ConnId) -> Option<usize> {
        self.spectators.iter().position(|s| s.conn == conn)
    }

    /// Returns whether the room list changed (the game started).
    pub fn tick_countdown(&mut self, dt: f32, outgoing: &mut Vec<(ConnId, Vec<u8>)>) -> bool {
        let Phase::CountingDown(t) = &mut self.phase else {
            return false;
        };
        let before = t.ceil() as u8;
        *t -= dt;
        if *t <= 0.0 {
            self.phase = Phase::Playing;
            self.start_game();
            outgoing.extend(self.game_start_payloads());
            return true;
        }
        if t.ceil() as u8 != before {
            outgoing.extend(self.lobby_payloads());
        }
        false
    }

    /// Advances a running game by one step and sends its state. Returns the
    /// match record when this step ended the game.
    /// Advances a running game by one step and sends its state. The last
    /// state goes with the reliable messages, since no other comes after
    /// it. `split(conn)` tells a link that takes states board by board.
    /// Returns the match record when this step ended the game.
    pub fn tick_game(
        &mut self,
        do_broadcast: bool,
        split: impl Fn(ConnId) -> bool,
        states: &mut Vec<(ConnId, StateOut)>,
        outgoing: &mut Vec<(ConnId, Vec<u8>)>,
    ) -> Option<db::MatchRecord> {
        if !matches!(self.phase, Phase::Playing) {
            return None;
        }
        let advanced = !self.sim.paused && !self.sim.finished;
        let mut record = None;
        if advanced {
            self.sim.advance();
            let alive = self.sim.teams_alive();
            if alive.len() <= 1 {
                self.sim.finished = true;
                let rec = self.match_record();
                info!(
                    "Match terminé room #{} → {} ({:.0}s, inputs en retard {:?})",
                    self.id,
                    rec.winner_slot()
                        .map_or_else(|| "égalité".to_string(), |w| format!("slot {w} gagne")),
                    rec.duration_secs,
                    self.sim.late_inputs
                );
                record = Some(rec);
            }
        }
        if record.is_some() {
            self.send_to_audience(&shared::encode(&self.sim.state_update()), outgoing);
        } else if do_broadcast && advanced {
            self.send_states(split, states);
        }
        record
    }

    /// From `SPARSE_FROM` boards, players get their own board every tick and
    /// the others as thumbnails one tick in `OTHERS_EVERY`; those without a
    /// board the same, with the one they follow as their own. Below, everyone
    /// gets every board whole. Each message shared is made once.
    fn send_states(&mut self, split: impl Fn(ConnId) -> bool, sink: &mut Vec<(ConnId, StateOut)>) {
        let boards = self.sim.boards.len();
        let sparse = boards >= SPARSE_FROM;
        let thumbs = (sparse && self.sim.tick.is_multiple_of(OTHERS_EVERY)).then(|| self.sim.thumbs());
        let sim = &self.sim;
        let (mut whole, mut all, mut small, mut small_packed) = (None, None, None, None);
        let followed = |conn| self.follows.get(&conn).copied().unwrap_or(0);
        let seated = self
            .members
            .iter()
            .filter_map(|m| Some((m.conn?, m.game_slot.filter(|&s| s < boards))));
        let watching = self.spectators.iter().map(|s| (s.conn, None));
        let audience = seated.chain(watching).map(|(conn, own)| {
            let large = own.unwrap_or_else(|| followed(conn));
            (conn, sparse.then_some(large))
        });
        for (conn, own) in audience {
            let datagrams = split(conn);
            let Some(slot) = own else {
                let out = if datagrams {
                    StateOut::Boards(
                        all.get_or_insert_with(|| {
                            let mut out = Vec::new();
                            packed(0..boards, &|r| shared::encode(&sim.state_update_of(r)), &mut out);
                            out
                        })
                        .clone(),
                    )
                } else {
                    StateOut::Whole(whole.get_or_insert_with(|| shared::encode(&sim.state_update())).clone())
                };
                sink.push((conn, out));
                continue;
            };
            let mine = shared::encode(&sim.state_update_of([slot]));
            let Some(
                msg @ ServerMessage::Thumbs {
                    game,
                    tick,
                    thumbs: list,
                },
            ) = &thumbs
            else {
                sink.push((
                    conn,
                    if datagrams {
                        StateOut::Boards(vec![Bytes::from(mine)])
                    } else {
                        StateOut::Whole(mine)
                    },
                ));
                continue;
            };
            if datagrams {
                let others = small_packed.get_or_insert_with(|| {
                    let mut out = Vec::new();
                    let encode = |r: Range<usize>| {
                        shared::encode(&ServerMessage::Thumbs {
                            game: *game,
                            tick: *tick,
                            thumbs: list[r].to_vec(),
                        })
                    };
                    packed(0..list.len(), &encode, &mut out);
                    out
                });
                let mut out = vec![Bytes::from(mine)];
                out.extend(others.iter().cloned());
                sink.push((conn, StateOut::Boards(out)));
            } else {
                sink.push((conn, StateOut::Whole(mine)));
                let others = small.get_or_insert_with(|| shared::encode(msg));
                sink.push((conn, StateOut::Whole(others.clone())));
            }
        }
    }

    pub fn slot_of_conn(&self, conn: ConnId) -> Option<usize> {
        self.members.iter().position(|m| m.conn == Some(conn))
    }

    pub fn is_host_conn(&self, conn: ConnId) -> bool {
        self.slot_of_conn(conn)
            .is_some_and(|s| self.members[s].token == self.host)
    }

    pub fn all_connected(&self) -> bool {
        self.members.iter().all(|m| m.conn.is_some())
    }

    /// Everyone who sees the room: its connected players, then its spectators.
    pub fn audience(&self) -> Vec<ConnId> {
        let players = self.members.iter().filter_map(|m| m.conn);
        players.chain(self.spectators.iter().map(|s| s.conn)).collect()
    }

    pub fn lobby_info_for(&self, idx: usize) -> LobbyInfo {
        LobbyInfo {
            your_slot: self.your_slot(idx),
            is_host: self.members[idx].token == self.host,
            ranked: self.series.as_ref().map(|s| RankedInfo {
                opponent: s.names[1 - idx].clone(),
                opponent_elo: s.elos[1 - idx],
                opponent_avatar: s.avatars[1 - idx].clone(),
                wins: s.wins,
            }),
            ..self.spectator_info()
        }
    }

    /// The lobby as someone without a seat sees it.
    pub fn spectator_info(&self) -> LobbyInfo {
        LobbyInfo {
            id: self.id,
            name: self.name.clone(),
            settings: self.settings,
            players: self.members.len() as u8,
            connected: self.members.iter().filter(|m| m.conn.is_some()).count() as u8,
            your_slot: 0,
            is_host: false,
            countdown: match self.phase {
                Phase::CountingDown(t) => Some(t.ceil() as u8),
                _ => None,
            },
            ranked: None,
            names: self.members.iter().map(|m| m.name.clone()).collect(),
            teams: self.members.iter().map(|m| m.team).collect(),
            spectators: self.spectators.len() as u8,
        }
    }

    pub fn game_running(&self) -> bool {
        matches!(self.phase, Phase::Playing) && !self.sim.finished
    }

    fn match_record(&self) -> db::MatchRecord {
        let places = self.sim.places();
        let n = |v: &[u32], s: usize| v.get(s).copied().unwrap_or(0);
        let players = (0..self.sim.boards.len())
            .map(|s| db::PlayerRecord {
                user_id: self.seats.get(s).and_then(|seat| seat.user_id),
                slot: slot_byte(s + 1),
                team: self.sim.teams[s],
                place: places[s],
                won: self.sim.won(s),
                max_chain: n(&self.sim.max_chain, s),
                total_chains: n(&self.sim.total_chains, s),
                nuisance_sent: n(&self.sim.nuisance_sent, s),
                nuisance_received: n(&self.sim.nuisance_received, s),
                all_clears: n(&self.sim.all_clears, s),
                pieces_placed: n(&self.sim.pieces_placed, s),
            })
            .collect();
        let names =
            (0..self.sim.boards.len()).map(|s| self.seats.get(s).map_or_else(String::new, |seat| seat.name.clone()));
        db::MatchRecord {
            ranked: self.series.is_some(),
            duration_secs: self.sim.start.elapsed().as_secs_f64(),
            players,
            replay: Some(shared::encode(&self.sim.replay(names))),
        }
    }

    /// Takes `slot` out of a running game. The game ends when one team is
    /// left, which must still have someone connected to be credited.
    pub fn forfeit(&mut self, member: usize) -> Option<db::MatchRecord> {
        let slot = self.members.get(member)?.game_slot?;
        if !self.game_running() || slot >= self.sim.boards.len() {
            return None;
        }
        let team = self.sim.teams[slot];
        let others_connected = self.members.iter().any(|m| {
            m.conn.is_some()
                && m.game_slot
                    .is_some_and(|s| s < self.sim.teams.len() && self.sim.teams[s] != team)
        });
        if !others_connected {
            return None;
        }
        self.sim.eliminate(slot);
        let alive = self.sim.teams_alive();
        if alive.len() > 1 {
            return None;
        }
        self.sim.finished = true;
        Some(self.match_record())
    }

    /// The game's team of each member: the one picked, or one of their own.
    pub fn teams(&self) -> Vec<u8> {
        self.members
            .iter()
            .enumerate()
            .map(|(i, m)| m.team.unwrap_or(shared::MAX_TEAMS + 1 + slot_byte(i)))
            .collect()
    }

    /// From 1 while the member plays the running game or waits in the lobby,
    /// 0 for one watching a game they came into late.
    fn your_slot(&self, member: usize) -> u8 {
        match (&self.phase, self.members[member].game_slot) {
            (Phase::Playing, Some(slot)) => slot_byte(slot + 1),
            (Phase::Playing, None) => 0,
            _ => slot_byte(member + 1),
        }
    }

    pub fn host_user(&self) -> Option<Uuid> {
        self.members
            .iter()
            .find(|m| m.token == self.host)
            .and_then(|m| m.user_id)
    }

    pub fn info(&self) -> RoomInfo {
        RoomInfo {
            id: self.id,
            name: self.name.clone(),
            players: self.members.len() as u8,
            max: self.settings.max_players,
            in_game: !matches!(self.phase, Phase::Lobby),
            friends_only: self.settings.friends_only,
            spectators: self.spectators.len() as u8,
            ranked: self.series.is_some(),
        }
    }
}
