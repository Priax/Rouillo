use shared::{GameState, LobbyInfo, RankedInfo, RoomId, RoomInfo, RoomSettings, ServerMessage};
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
}

impl Member {
    pub const fn present(token: Token, conn: ConnId, user_id: Option<Uuid>) -> Self {
        Self {
            token,
            conn: Some(conn),
            disconnect_at: None,
            user_id,
        }
    }
}

pub struct Series {
    pub users: [Uuid; 2],
    pub names: [String; 2],
    pub elos: [i32; 2],
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
}

impl Room {
    pub fn new(id: RoomId, name: String, members: Vec<Member>, settings: RoomSettings, series: Option<Series>) -> Self {
        Self {
            id,
            name,
            host: members[0].token.clone(),
            members,
            settings,
            phase: Phase::Lobby,
            sim: Sim::new(&settings),
            series,
        }
    }

    pub fn back_to_lobby(&mut self) {
        self.phase = Phase::Lobby;
        self.sim.finished = false;
        self.sim.set_paused(false);
    }

    pub fn lobby_payloads(&self) -> Vec<(ConnId, Vec<u8>)> {
        self.members
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let msg = ServerMessage::Lobby {
                    info: self.lobby_info_for(i),
                };
                match shared::encode(&msg) {
                    Ok(payload) => Some((m.conn?, payload)),
                    Err(e) => {
                        error!("encode Lobby failed: {e}");
                        None
                    }
                }
            })
            .collect()
    }

    fn send_to_members(&self, payload: &[u8], outgoing: &mut Vec<(ConnId, Vec<u8>)>) {
        for m in &self.members {
            if let Some(c) = m.conn {
                outgoing.push((c, payload.to_vec()));
            }
        }
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
            self.sim.reset_boards(&self.settings);
            match shared::encode(&ServerMessage::GameStart) {
                Ok(payload) => self.send_to_members(&payload, outgoing),
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
            if self.sim.boards.iter().any(|b| b.state == GameState::GameOver) {
                self.sim.finished = true;
                let lost = self.sim.boards.each_ref().map(|b| b.state == GameState::GameOver);
                let winner_slot = match lost {
                    [true, true] => None,
                    [true, false] => Some(2),
                    _ => Some(1),
                };
                let rec = self.match_record(winner_slot);
                info!(
                    "Match terminé room #{} → {} ({:.0}s, inputs en retard {:?})",
                    self.id,
                    winner_slot.map_or_else(|| "égalité".to_string(), |w| format!("slot {w} gagne")),
                    rec.duration_secs,
                    self.sim.late_inputs
                );
                record = Some(rec);
            }
        }
        let just_finished = record.is_some();
        if (do_broadcast && advanced) || just_finished {
            let msg = self.sim.state_update(just_finished);
            self.sim.last_sent_rng = Some(self.sim.rng_positions());
            match shared::encode(&msg) {
                Ok(upd) => self.send_to_members(&upd, outgoing),
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

    pub fn connected_conns(&self) -> Vec<ConnId> {
        self.members.iter().filter_map(|m| m.conn).collect()
    }

    pub fn lobby_info_for(&self, idx: usize) -> LobbyInfo {
        LobbyInfo {
            id: self.id,
            name: self.name.clone(),
            settings: self.settings,
            players: self.members.len() as u8,
            connected: self.members.iter().filter(|m| m.conn.is_some()).count() as u8,
            your_slot: (idx + 1) as u8,
            is_host: self.members[idx].token == self.host,
            countdown: match self.phase {
                Phase::CountingDown(t) => Some(t.ceil() as u8),
                _ => None,
            },
            ranked: self.series.as_ref().map(|s| RankedInfo {
                opponent: s.names[1 - idx].clone(),
                opponent_elo: s.elos[1 - idx],
                wins: s.wins,
            }),
        }
    }

    pub fn game_running(&self) -> bool {
        matches!(self.phase, Phase::Playing) && !self.sim.finished
    }

    fn match_record(&self, winner_slot: Option<u8>) -> db::MatchRecord {
        db::MatchRecord {
            ranked: self.series.is_some(),
            duration_secs: self.sim.start.elapsed().as_secs_f64(),
            winner_slot,
            user_ids: [
                self.members.first().and_then(|m| m.user_id),
                self.members.get(1).and_then(|m| m.user_id),
            ],
            max_chain: self.sim.max_chain,
            total_chains: self.sim.total_chains,
            nuisance_sent: self.sim.nuisance_sent,
            all_clears: self.sim.all_clears,
            pieces_placed: self.sim.pieces_placed,
        }
    }

    pub fn forfeit(&mut self, slot: usize) -> Option<db::MatchRecord> {
        if !self.game_running() || self.members.len() != 2 || slot > 1 {
            return None;
        }
        let winner = 1 - slot;
        self.members[winner].conn?;
        self.sim.finished = true;
        Some(self.match_record(Some((winner + 1) as u8)))
    }

    pub fn info(&self) -> RoomInfo {
        RoomInfo {
            id: self.id,
            name: self.name.clone(),
            players: self.members.len() as u8,
            max: 2,
            in_game: !matches!(self.phase, Phase::Lobby),
            friends_only: self.settings.friends_only,
        }
    }
}
