use super::*;

pub const MIN_CASUAL_GAMES: i64 = 5;
pub const NEXT_GAME_DELAY: Duration = Duration::from_secs(4);
const QUEUE_WINDOW_BASE: f32 = 100.0;
const QUEUE_WINDOW_PER_SEC: f32 = 20.0;
const QUEUE_WINDOW_MAX: f32 = 1000.0;

const QUEUE_GUEST: &str = "Connectez-vous pour jouer en classé.";
const QUEUE_BUSY: &str = "Ce compte est déjà en classé.";
const QUEUE_UNKNOWN: &str = "Compte introuvable.";

pub struct QueueEntry {
    pub conn: ConnId,
    pub user_id: Uuid,
    pub elo: i32,
    pub since: Instant,
}

#[derive(Debug, Clone, Copy)]
pub struct RankedCheck {
    pub conn: ConnId,
    pub user_id: Uuid,
}

fn queue_window(waited: Duration) -> f32 {
    (QUEUE_WINDOW_BASE + QUEUE_WINDOW_PER_SEC * waited.as_secs_f32()).min(QUEUE_WINDOW_MAX)
}

impl Manager {
    pub fn take_unsaved_series(&mut self) -> Vec<(Uuid, Uuid)> {
        std::mem::take(&mut self.unsaved_series)
    }

    pub fn take_ranked_checks(&mut self) -> Vec<RankedCheck> {
        std::mem::take(&mut self.ranked_checks)
    }

    pub(super) fn attach_ranked(&mut self, id: RoomId, conn: ConnId) {
        let Some(room) = self.rooms.get(&id) else { return };
        let Some(slot) = room.slot_of_conn(conn) else { return };
        let lobby = ServerMessage::Lobby {
            info: room.lobby_info_for(slot),
        };
        let playing = matches!(room.phase, Phase::Playing);
        let snapshot = playing.then(|| room.sim.state_update(true));
        let result = room.series.as_ref().and_then(|s| s.result);
        self.deliver_msg(conn, &lobby);
        if let Some(snapshot) = snapshot {
            self.deliver_msg(conn, &ServerMessage::GameStart);
            self.deliver_msg(conn, &snapshot);
        }
        if let Some(result) = result {
            self.send_series_over(conn, result, slot);
        }
    }

    pub(super) fn refuse_queue(&mut self, conn: ConnId, reason: &str) {
        self.deliver_msg(
            conn,
            &ServerMessage::QueueRefused {
                reason: reason.to_string(),
            },
        );
    }

    pub(super) fn in_ranked(&self, user: Uuid) -> bool {
        self.queue.iter().any(|e| e.user_id == user)
            || self.rooms.values().any(|r| {
                r.series
                    .as_ref()
                    .is_some_and(|s| s.result.is_none() && s.users.contains(&user))
            })
    }

    pub(super) fn join_queue(&mut self, conn: ConnId) {
        let Some(user_id) = self.conn_user_id.get(&conn).copied() else {
            self.refuse_queue(conn, QUEUE_GUEST);
            return;
        };
        if self
            .room_of(conn)
            .is_some_and(|id| self.series(id).is_some_and(|s| s.result.is_none()))
        {
            return;
        }
        if self.closing {
            self.refuse_queue(conn, JOIN_MAINTENANCE);
            return;
        }
        if self.in_ranked(user_id) {
            self.refuse_queue(conn, QUEUE_BUSY);
            return;
        }
        if !self.conn_token.contains_key(&conn) || !self.ranked_checking.insert(conn) {
            return;
        }
        self.ranked_checks.push(RankedCheck { conn, user_id });
    }

    pub(super) fn ranked_check_done(&mut self, check: RankedCheck, profile: Option<(i32, i64)>) {
        let RankedCheck { conn, user_id } = check;
        self.ranked_checking.remove(&conn);
        if self.conn_user_id.get(&conn) != Some(&user_id) {
            return;
        }
        let Some((elo, casual)) = profile else {
            self.refuse_queue(conn, QUEUE_UNKNOWN);
            return;
        };
        if casual < MIN_CASUAL_GAMES {
            let left = MIN_CASUAL_GAMES - casual;
            let plural = if left > 1 { "s" } else { "" };
            self.refuse_queue(
                conn,
                &format!("Jouez encore {left} partie{plural} amicale{plural} avant le classé."),
            );
            return;
        }
        if self.closing {
            self.refuse_queue(conn, JOIN_MAINTENANCE);
            return;
        }
        if self.in_ranked(user_id) {
            self.refuse_queue(conn, QUEUE_BUSY);
            return;
        }
        self.leave_current(conn);
        self.queue.push(QueueEntry {
            conn,
            user_id,
            elo,
            since: Instant::now(),
        });
    }

    pub(super) fn matchmake(&mut self) {
        if self.queue.len() < 2 || self.closing {
            return;
        }
        let now = Instant::now();
        self.queue.sort_by_key(|e| e.since);
        let mut taken = vec![false; self.queue.len()];
        let mut pairs = Vec::new();
        for i in 0..self.queue.len() {
            if taken[i] {
                continue;
            }
            let a = &self.queue[i];
            let best = (i + 1..self.queue.len())
                .filter(|&j| !taken[j] && self.queue[j].user_id != a.user_id)
                .map(|j| (j, (a.elo - self.queue[j].elo).unsigned_abs()))
                .filter(|&(_, gap)| gap as f32 <= queue_window(now.duration_since(a.since)))
                .min_by_key(|&(_, gap)| gap);
            if let Some((j, _)) = best {
                taken[i] = true;
                taken[j] = true;
                pairs.push((i, j));
            }
        }
        let mut entries: Vec<Option<QueueEntry>> = std::mem::take(&mut self.queue).into_iter().map(Some).collect();
        for (i, j) in pairs {
            if let (Some(a), Some(b)) = (entries[i].take(), entries[j].take()) {
                self.start_series(&a, &b);
            }
        }
        self.queue = entries.into_iter().flatten().collect();
    }

    pub(super) fn start_series(&mut self, a: &QueueEntry, b: &QueueEntry) {
        let (Some(token_a), Some(token_b)) = (
            self.conn_token.get(&a.conn).cloned(),
            self.conn_token.get(&b.conn).cloned(),
        ) else {
            return;
        };
        let name = |conn| {
            self.conn_username
                .get(&conn)
                .cloned()
                .unwrap_or_else(|| "Joueur".to_string())
        };
        let names = [name(a.conn), name(b.conn)];
        self.leave_current(a.conn);
        self.leave_current(b.conn);
        let id = self.next_id;
        self.next_id += 1;
        let settings = RoomSettings {
            pause: PausePolicy::Nobody,
            ..RoomSettings::default()
        };
        let member = |token, conn, user| Member {
            token,
            conn: Some(conn),
            disconnect_at: None,
            user_id: Some(user),
        };
        let room = Room {
            id,
            name: "Classé".to_string(),
            host: token_a.clone(),
            members: vec![member(token_a, a.conn, a.user_id), member(token_b, b.conn, b.user_id)],
            settings,
            phase: Phase::CountingDown(3.0),
            sim: Sim::new(&settings),
            series: Some(Series {
                users: [a.user_id, b.user_id],
                names,
                elos: [a.elo, b.elo],
                wins: [0, 0],
                next_game_at: None,
                result: None,
            }),
        };
        self.rooms.insert(id, room);
        self.clients.insert(a.conn, Some(id));
        self.clients.insert(b.conn, Some(id));
        self.send_lobby(id);
        info!("Série classée room #{id} ({} contre {})", a.elo, b.elo);
    }

    pub(super) fn series_game_won(&mut self, id: RoomId, winner: Option<usize>) {
        let Some(series) = self.rooms.get_mut(&id).and_then(|r| r.series.as_mut()) else {
            return;
        };
        if series.result.is_some() {
            return;
        }
        let Some(winner) = winner.filter(|&w| w <= 1) else {
            series.next_game_at = Some(Instant::now() + NEXT_GAME_DELAY);
            return;
        };
        series.wins[winner] += 1;
        let wins = series.wins;
        self.send_room_msg(id, &ServerMessage::SeriesScore { wins });
        if wins[winner] >= config::RANKED_WINS {
            self.finish_series(id, Some(winner));
        } else if let Some(series) = self.rooms.get_mut(&id).and_then(|r| r.series.as_mut()) {
            series.next_game_at = Some(Instant::now() + NEXT_GAME_DELAY);
        }
    }

    pub(super) fn advance_series(&mut self) {
        let now = Instant::now();
        let due: Vec<RoomId> = self
            .rooms
            .values()
            .filter(|r| {
                r.series
                    .as_ref()
                    .is_some_and(|s| s.result.is_none() && s.next_game_at.is_some_and(|t| now >= t))
                    && r.all_connected()
            })
            .map(|r| r.id)
            .collect();
        for id in due {
            if self.closing {
                self.finish_series(id, None);
                continue;
            }
            if let Some(room) = self.rooms.get_mut(&id) {
                room.sim.reset_boards(&room.settings);
                if let Some(series) = room.series.as_mut() {
                    series.next_game_at = None;
                }
            }
            self.send_room_msg(id, &ServerMessage::Restart);
        }
    }

    pub(super) fn finish_series(&mut self, id: RoomId, winner: Option<usize>) {
        let Some(series) = self.rooms.get_mut(&id).and_then(|r| r.series.as_mut()) else {
            return;
        };
        if series.result.is_some() {
            return;
        }
        let elo_changes = winner.map_or([0, 0], |w| {
            let new = db::compute_elo(series.elos[0], series.elos[1], w);
            [new[0] - series.elos[0], new[1] - series.elos[1]]
        });
        let result = SeriesResult { winner, elo_changes };
        series.result = Some(result);
        series.next_game_at = None;
        if let Some(w) = winner {
            self.unsaved_series.push((series.users[w], series.users[1 - w]));
            info!("Série room #{id} terminée, slot {} gagne", w + 1);
        }
        let conns: Vec<(usize, ConnId)> = self
            .rooms
            .get(&id)
            .map(|r| {
                r.members
                    .iter()
                    .enumerate()
                    .filter_map(|(i, m)| Some((i, m.conn?)))
                    .collect()
            })
            .unwrap_or_default();
        for (slot, conn) in conns {
            self.send_series_over(conn, result, slot);
        }
    }

    pub(super) fn send_series_over(&mut self, conn: ConnId, result: SeriesResult, slot: usize) {
        self.deliver_msg(
            conn,
            &ServerMessage::SeriesOver {
                winner_slot: result.winner.map(|w| (w + 1) as u8),
                elo_change: result.elo_changes[slot],
            },
        );
    }

    pub(super) fn forfeit_series(&mut self, id: RoomId, slot: usize) {
        let running = self.series(id).is_some_and(|s| s.result.is_none());
        if running && slot <= 1 {
            if let Some(room) = self.rooms.get_mut(&id) {
                room.sim.finished = true;
            }
            self.finish_series(id, Some(1 - slot));
        }
    }

    pub(super) fn series(&self, id: RoomId) -> Option<&Series> {
        self.rooms.get(&id).and_then(|r| r.series.as_ref())
    }

    pub(super) fn series_over(&self, id: RoomId) -> bool {
        self.series(id).is_some_and(|s| s.result.is_some())
    }

    pub(super) fn detach_ranked(&mut self, id: RoomId, slot: usize) {
        let Some(room) = self.rooms.get_mut(&id) else { return };
        if let Some(member) = room.members.get_mut(slot) {
            member.conn = None;
            member.disconnect_at = None;
            member.token.clear();
        }
        if room.members.iter().all(|m| m.conn.is_none()) {
            self.rooms.remove(&id);
        }
    }
}
