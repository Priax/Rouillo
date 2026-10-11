use shared::{LobbyInfo, RankedInfo, RoomId, RoomInfo, RoomSettings, ServerMessage};
use tokio::time::Instant;
use tracing::{error, info};
use uuid::Uuid;

use crate::sim::Sim;
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
}

impl Member {
    pub const fn present(token: Token, conn: ConnId, user_id: Option<Uuid>, name: String) -> Self {
        Self {
            token,
            conn: Some(conn),
            disconnect_at: None,
            user_id,
            name,
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
            sim: Sim::new(&settings, (0..members_len).map(team_of).collect()),
            series,
            spectators: Vec::new(),
        }
    }

    pub fn back_to_lobby(&mut self) {
        self.phase = Phase::Lobby;
        self.sim.finished = false;
        self.sim.set_paused(false);
    }

    pub fn lobby_payloads(&self) -> Vec<(ConnId, Vec<u8>)> {
        let encode = |info: LobbyInfo| match shared::encode(&ServerMessage::Lobby { info }) {
            Ok(payload) => Some(payload),
            Err(e) => {
                error!("encode Lobby failed: {e}");
                None
            }
        };
        let members = self
            .members
            .iter()
            .enumerate()
            .filter_map(|(i, m)| Some((m.conn?, encode(self.lobby_info_for(i))?)));
        let spectators = encode(self.spectator_info())
            .map(|payload| self.spectators.iter().map(move |s| (s.conn, payload.clone())))
            .into_iter()
            .flatten();
        members.chain(spectators).collect()
    }

    fn send_to_audience(&self, payload: &[u8], outgoing: &mut Vec<(ConnId, Vec<u8>)>) {
        for c in self.audience() {
            outgoing.push((c, payload.to_vec()));
        }
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
            self.sim.reset_boards(&self.settings, self.teams());
            match shared::encode(&ServerMessage::GameStart) {
                Ok(payload) => self.send_to_audience(&payload, outgoing),
                Err(e) => error!("encode GameStart failed: {e}"),
            }
            return true;
        }
        if t.ceil() as u8 != before {
            outgoing.extend(self.lobby_payloads());
        }
        false
    }

    /// Advances a running game by one step and sends its state. Returns the
    /// match record when this step ended the game.
    pub fn tick_game(&mut self, do_broadcast: bool, outgoing: &mut Vec<(ConnId, Vec<u8>)>) -> Option<db::MatchRecord> {
        if !matches!(self.phase, Phase::Playing) {
            return None;
        }
        let advanced = !self.sim.paused && !self.sim.finished;
        let mut record = None;
        if advanced {
            self.sim.advance();
            self.sim.record_stats();
            let alive = self.sim.teams_alive();
            if alive.len() <= 1 {
                self.sim.finished = true;
                let winner_slot = alive
                    .first()
                    .and_then(|&team| (0..self.sim.boards.len()).find(|&s| self.sim.teams[s] == team));
                info!(
                    "Match terminé room #{} → {} ({:.0}s, inputs en retard {:?})",
                    self.id,
                    winner_slot.map_or_else(|| "égalité".to_string(), |w| format!("slot {} gagne", w + 1)),
                    self.sim.start.elapsed().as_secs_f64(),
                    self.sim.late_inputs
                );
                record = self.match_record(winner_slot);
            }
        }
        let just_finished = record.is_some();
        if (do_broadcast && advanced) || just_finished {
            let msg = self.sim.state_update(just_finished);
            self.sim.last_sent_rng = Some(self.sim.rng_positions());
            match shared::encode(&msg) {
                Ok(upd) => self.send_to_audience(&upd, outgoing),
                Err(e) => error!("encode StateUpdate failed: {e}"),
            }
        }
        record
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
            your_slot: (idx + 1) as u8,
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
            spectators: self.spectators.len() as u8,
        }
    }

    pub fn game_running(&self) -> bool {
        matches!(self.phase, Phase::Playing) && !self.sim.finished
    }

    /// Only two-player games are recorded until the database knows more.
    fn match_record(&self, winner: Option<usize>) -> Option<db::MatchRecord> {
        if self.sim.boards.len() != 2 {
            return None;
        }
        let pair = |v: &[u32]| [v[0], v[1]];
        Some(db::MatchRecord {
            ranked: self.series.is_some(),
            duration_secs: self.sim.start.elapsed().as_secs_f64(),
            winner_slot: winner.and_then(|w| u8::try_from(w + 1).ok()),
            user_ids: [
                self.members.first().and_then(|m| m.user_id),
                self.members.get(1).and_then(|m| m.user_id),
            ],
            max_chain: pair(&self.sim.max_chain),
            total_chains: pair(&self.sim.total_chains),
            nuisance_sent: pair(&self.sim.nuisance_sent),
            all_clears: pair(&self.sim.all_clears),
            pieces_placed: pair(&self.sim.pieces_placed),
        })
    }

    /// Takes `slot` out of a running game. The game ends when one team is
    /// left, which must still have someone connected to be credited.
    pub fn forfeit(&mut self, slot: usize) -> Option<db::MatchRecord> {
        if !self.game_running() || slot >= self.sim.boards.len() {
            return None;
        }
        let team = self.sim.teams[slot];
        let others_connected = (0..self.sim.boards.len())
            .any(|s| self.sim.teams[s] != team && self.members.get(s).is_some_and(|m| m.conn.is_some()));
        if !others_connected {
            return None;
        }
        self.sim.eliminate(slot);
        let alive = self.sim.teams_alive();
        if alive.len() > 1 {
            return None;
        }
        self.sim.finished = true;
        let winner = alive
            .first()
            .and_then(|&t| (0..self.sim.boards.len()).find(|&s| self.sim.teams[s] == t));
        self.match_record(winner)
    }

    /// One team per player, until the lobby lets them pick.
    pub fn teams(&self) -> Vec<u8> {
        (0..self.members.len()).map(team_of).collect()
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
            max: 2,
            in_game: !matches!(self.phase, Phase::Lobby),
            friends_only: self.settings.friends_only,
            spectators: self.spectators.len() as u8,
            ranked: self.series.is_some(),
        }
    }
}

fn team_of(slot: usize) -> u8 {
    u8::try_from(slot).unwrap_or(u8::MAX)
}
