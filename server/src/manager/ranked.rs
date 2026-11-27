use super::*;

pub const NEXT_GAME_DELAY: Duration = Duration::from_secs(4);
const QUEUE_WINDOW_BASE: f32 = 100.0;
const QUEUE_WINDOW_PER_SEC: f32 = 20.0;
const QUEUE_WINDOW_MAX: f32 = 1000.0;

const QUEUE_GUEST: &str = "Connectez-vous pour jouer en classé.";
const QUEUE_BUSY: &str = "Ce compte est déjà en classé.";
const QUEUE_UNKNOWN: &str = "Compte introuvable.";

pub const ACCEPT_WINDOW: Duration = Duration::from_secs(config::MATCH_ACCEPT_SECS as u64);
pub const QUEUE_COOLDOWN: Duration = Duration::from_secs(config::QUEUE_COOLDOWN_SECS as u64);

pub struct QueueEntry {
    pub conn: ConnId,
    pub user_id: Uuid,
    pub elo: i32,
    pub avatar: Option<String>,
    pub since: Instant,
}

pub struct PendingMatch {
    pub entries: [QueueEntry; 2],
    pub accepted: [bool; 2],
    pub deadline: Instant,
}

#[derive(Debug, Clone, Copy)]
pub struct RankedCheck {
    pub conn: ConnId,
    pub user_id: Uuid,
}

fn queue_window(waited: Duration) -> f32 {
    (QUEUE_WINDOW_BASE + QUEUE_WINDOW_PER_SEC * waited.as_secs_f32()).min(QUEUE_WINDOW_MAX)
}

/// Those waiting for a ranked opponent, the matches waiting to be accepted,
/// and those who let one lapse, kept out for a while.
#[derive(Default)]
pub struct Ranked {
    pub queue: Vec<QueueEntry>,
    pub pending: Vec<PendingMatch>,
    cooldowns: HashMap<Uuid, Instant>,
}

impl Ranked {
    fn waiting(&self, user: Uuid) -> bool {
        self.queue.iter().any(|e| e.user_id == user)
            || self.pending.iter().any(|m| m.entries.iter().any(|e| e.user_id == user))
    }

    /// Takes out of the queue each pair close enough in ELO, the longest
    /// waiting first; the window widens with the wait.
    fn pair_up(&mut self, now: Instant) -> Vec<[QueueEntry; 2]> {
        self.queue.sort_by_key(|e| e.since);
        let mut waiting: Vec<Option<QueueEntry>> = self.queue.drain(..).map(Some).collect();
        let mut pairs = Vec::new();
        for i in 0..waiting.len() {
            let Some(a) = &waiting[i] else { continue };
            let window = queue_window(now.duration_since(a.since));
            let best = (i + 1..waiting.len())
                .filter_map(|j| {
                    waiting[j].as_ref().filter(|b| b.user_id != a.user_id).map(|b| (j, a.elo.abs_diff(b.elo)))
                })
                .filter(|&(_, gap)| gap as f32 <= window)
                .min_by_key(|&(_, gap)| gap);
            if let Some((j, _)) = best {
                if let (Some(a), Some(b)) = (waiting[i].take(), waiting[j].take()) {
                    pairs.push([a, b]);
                }
            }
        }
        self.queue.extend(waiting.into_iter().flatten());
        pairs
    }

    fn pending_of(&self, conn: ConnId) -> Option<(usize, usize)> {
        self.pending
            .iter()
            .enumerate()
            .find_map(|(i, m)| m.entries.iter().position(|e| e.conn == conn).map(|side| (i, side)))
    }

    fn cooldown_left(&self, user: Uuid) -> Option<Duration> {
        let until = *self.cooldowns.get(&user)?;
        let now = Instant::now();
        (until > now).then(|| until - now)
    }
}

impl Manager {
    pub(super) fn attach_ranked(&mut self, id: RoomId, conn: ConnId) {
        let Some(room) = self.rooms.get(&id) else { return };
        let Some(slot) = room.slot_of_conn(conn) else { return };
        let lobby = ServerMessage::Lobby { info: room.lobby_info_for(slot) };
        let playing = matches!(room.phase, Phase::Playing);
        let snapshot = playing.then(|| room.sim.state_update());
        let result = room.series.as_ref().and_then(|s| s.result);
        self.clients.deliver_msg(conn, &lobby);
        if let Some(snapshot) = snapshot {
            self.deliver_game_start(id, conn);
            self.clients.deliver_msg(conn, &snapshot);
        }
        if let Some(result) = result {
            self.clients.deliver_msg(conn, &series_over(result, slot));
        }
    }

    pub(super) fn refuse_queue(&mut self, conn: ConnId, reason: &str) {
        self.clients.deliver_msg(conn, &ServerMessage::QueueRefused { reason: reason.to_string() });
    }

    pub(super) fn in_ranked(&self, user: Uuid) -> bool {
        self.ranked.waiting(user)
            || self
                .rooms
                .values()
                .any(|r| r.series.as_ref().is_some_and(|s| s.result.is_none() && s.users.contains(&user)))
    }

    pub(super) fn join_queue(&mut self, conn: ConnId) {
        let Some(user_id) = self.clients.user_of(conn) else {
            self.refuse_queue(conn, QUEUE_GUEST);
            return;
        };
        if self.clients.room_of(conn).is_some_and(|id| self.series(id).is_some_and(|s| s.result.is_none())) {
            return;
        }
        if let Some(reason) = self.queue_blocked(user_id) {
            self.refuse_queue(conn, reason);
            return;
        }
        if let Some(left) = self.ranked.cooldown_left(user_id) {
            self.send_cooldown(conn, left);
            return;
        }
        match self.clients.get_mut(conn) {
            Some(client) if client.token.is_some() && !client.checking_ranked => client.checking_ranked = true,
            _ => return,
        }
        self.effects.push(Effect::CheckRanked(RankedCheck { conn, user_id }));
    }

    /// Checked again once the database answered: either may have changed.
    fn queue_blocked(&self, user: Uuid) -> Option<&'static str> {
        if self.closing {
            Some(JOIN_MAINTENANCE)
        } else if self.in_ranked(user) {
            Some(QUEUE_BUSY)
        } else {
            None
        }
    }

    pub(super) fn ranked_check_done(&mut self, check: RankedCheck, profile: Option<db::RankedProfile>) {
        let RankedCheck { conn, user_id } = check;
        if let Some(client) = self.clients.get_mut(conn) {
            client.checking_ranked = false;
        }
        if self.clients.user_of(conn) != Some(user_id) {
            return;
        }
        let Some(db::RankedProfile { elo, casual, avatar_url: avatar }) = profile else {
            self.refuse_queue(conn, QUEUE_UNKNOWN);
            return;
        };
        if casual < config::RANKED_MIN_CASUAL {
            let left = config::RANKED_MIN_CASUAL - casual;
            let plural = if left > 1 { "s" } else { "" };
            self.refuse_queue(conn, &format!("Jouez encore {left} partie{plural} amicale{plural} avant le classé."));
            return;
        }
        if let Some(reason) = self.queue_blocked(user_id) {
            self.refuse_queue(conn, reason);
            return;
        }
        self.leave_current(conn);
        self.ranked.queue.push(QueueEntry { conn, user_id, elo, avatar, since: Instant::now() });
    }

    pub(super) fn matchmake(&mut self) {
        if self.ranked.queue.len() < 2 || self.closing {
            return;
        }
        for pair in self.ranked.pair_up(Instant::now()) {
            self.propose_match(pair);
        }
    }

    fn propose_match(&mut self, entries: [QueueEntry; 2]) {
        for (side, entry) in entries.iter().enumerate() {
            let other = &entries[1 - side];
            let found = ServerMessage::MatchFound {
                opponent: self.clients.display_name(other.conn),
                opponent_elo: other.elo,
                opponent_avatar: other.avatar.clone(),
                secs: config::MATCH_ACCEPT_SECS,
            };
            self.clients.deliver_msg(entry.conn, &found);
        }
        self.ranked.pending.push(PendingMatch {
            entries,
            accepted: [false; 2],
            deadline: Instant::now() + ACCEPT_WINDOW,
        });
    }

    pub(super) fn accept_match(&mut self, conn: ConnId) {
        let Some((i, side)) = self.ranked.pending_of(conn) else {
            return;
        };
        let pending = &mut self.ranked.pending[i];
        pending.accepted[side] = true;
        if pending.accepted == [true, true] {
            let pending = self.ranked.pending.swap_remove(i);
            self.start_series(pending);
        }
    }

    pub(super) fn leave_queue(&mut self, conn: ConnId) {
        self.ranked.queue.retain(|e| e.conn != conn);
        if let Some((i, side)) = self.ranked.pending_of(conn) {
            let pending = self.ranked.pending.swap_remove(i);
            let mut missed = [false; 2];
            missed[side] = true;
            self.cancel_match(pending, missed);
        }
    }

    pub(super) fn expire_matches(&mut self) {
        let now = Instant::now();
        self.ranked.cooldowns.retain(|_, until| *until > now);
        let due: Vec<PendingMatch> = self.ranked.pending.extract_if(.., |m| now >= m.deadline).collect();
        for pending in due {
            let missed = pending.accepted.map(|a| !a);
            self.cancel_match(pending, missed);
        }
    }

    fn cancel_match(&mut self, pending: PendingMatch, missed: [bool; 2]) {
        for (entry, missed) in pending.entries.into_iter().zip(missed) {
            if missed {
                self.ranked.cooldowns.insert(entry.user_id, Instant::now() + QUEUE_COOLDOWN);
                self.clients.deliver_msg(entry.conn, &ServerMessage::MatchCancelled { requeued: false });
                self.send_cooldown(entry.conn, QUEUE_COOLDOWN);
            } else if self.clients.user_of(entry.conn) == Some(entry.user_id) {
                self.clients.deliver_msg(entry.conn, &ServerMessage::MatchCancelled { requeued: true });
                self.ranked.queue.push(entry);
            }
        }
    }

    /// Everyone waiting for a ranked game is told the server goes down.
    pub(super) fn refuse_queue_for_maintenance(&mut self) {
        let msg = encode(&ServerMessage::QueueRefused { reason: JOIN_MAINTENANCE.to_string() });
        let pending = self.ranked.pending.drain(..).flat_map(|m| m.entries);
        let waiting = self.ranked.queue.drain(..).chain(pending);
        self.clients.deliver_all(waiting.map(|e| e.conn), &msg);
    }

    fn send_cooldown(&mut self, conn: ConnId, left: Duration) {
        let secs = left.as_secs_f32().ceil() as u32;
        self.clients.deliver_msg(conn, &ServerMessage::QueueCooldown { secs });
    }

    /// A player gone from under the match misses it: the other goes back to the queue.
    fn start_series(&mut self, pending: PendingMatch) {
        let [a, b] = &pending.entries;
        let tokens = (self.clients.token_of(a.conn).cloned(), self.clients.token_of(b.conn).cloned());
        let (Some(token_a), Some(token_b)) = tokens else {
            let missed = [tokens.0.is_none(), tokens.1.is_none()];
            self.cancel_match(pending, missed);
            return;
        };
        let names = [self.clients.display_name(a.conn), self.clients.display_name(b.conn)];
        self.leave_current(a.conn);
        self.leave_current(b.conn);
        let id = self.next_id;
        self.next_id += 1;
        let settings = RoomSettings { pause: PausePolicy::Nobody, ..RoomSettings::default() };
        let members = vec![
            Member::present(token_a, a.conn, Some(a.user_id), names[0].clone()),
            Member::present(token_b, b.conn, Some(b.user_id), names[1].clone()),
        ];
        let series = Series {
            users: [a.user_id, b.user_id],
            names,
            elos: [a.elo, b.elo],
            avatars: [a.avatar.clone(), b.avatar.clone()],
            wins: [0, 0],
            next_game_at: None,
            result: None,
        };
        let mut room = Room::new(id, "Classé".to_string(), members, settings, Some(series));
        room.phase = Phase::CountingDown(COUNTDOWN_SECS);
        self.rooms.insert(id, room);
        self.clients.set_room(a.conn, Some(id));
        self.clients.set_room(b.conn, Some(id));
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
        let due = |r: &Room| {
            r.series.as_ref().is_some_and(|s| s.result.is_none() && s.next_game_at.is_some_and(|t| now >= t))
                && r.all_connected()
        };
        let Self { rooms, clients, effects, closing, .. } = self;
        for room in rooms.values_mut().filter(|r| due(r)) {
            if *closing {
                finish_series(room, None, effects, clients);
                continue;
            }
            room.start_game();
            if let Some(series) = room.series.as_mut() {
                series.next_game_at = None;
            }
            clients.deliver_many(room.game_start_payloads());
        }
    }

    pub(super) fn finish_series(&mut self, id: RoomId, winner: Option<usize>) {
        if let Some(room) = self.rooms.get_mut(&id) {
            finish_series(room, winner, &mut self.effects, &mut self.clients);
        }
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

    pub(crate) fn series_over(&self, id: RoomId) -> bool {
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
            self.close_room(id);
        }
    }
}

/// Ends `room`'s series, `winner` taking the ELO from the other, if any.
fn finish_series(room: &mut Room, winner: Option<usize>, effects: &mut Vec<Effect>, clients: &mut Clients) {
    let Some(series) = room.series.as_mut().filter(|s| s.result.is_none()) else {
        return;
    };
    let elo_changes = winner.map_or([0, 0], |w| {
        let new = db::compute_elo(series.elos[0], series.elos[1], w);
        [new[0] - series.elos[0], new[1] - series.elos[1]]
    });
    let result = SeriesResult { winner, elo_changes };
    series.result = Some(result);
    series.next_game_at = None;
    if let Some(w) = winner {
        effects.push(Effect::SaveSeries(db::SeriesRecord {
            winner: series.users[w],
            loser: series.users[1 - w],
            winner_delta: elo_changes[w],
            loser_delta: elo_changes[1 - w],
        }));
        info!("Série room #{} terminée, slot {} gagne", room.id, w + 1);
    }
    for (slot, conn) in room.members.iter().enumerate().filter_map(|(i, m)| Some((i, m.conn?))) {
        clients.deliver_msg(conn, &series_over(result, slot));
    }
}

fn series_over(result: SeriesResult, slot: usize) -> ServerMessage {
    ServerMessage::SeriesOver {
        winner_slot: result.winner.map(|w| (w + 1) as u8),
        elo_change: result.elo_changes[slot],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(conn: ConnId, user: u128, elo: i32, since: Instant) -> QueueEntry {
        QueueEntry { conn, user_id: Uuid::from_u128(user), elo, avatar: None, since }
    }

    #[test]
    fn the_longest_waiting_gets_the_closest_opponent_and_the_rest_keep_waiting() {
        let t = Instant::now();
        let queue = vec![
            entry(3, 3, 1050, t + Duration::from_secs(2)),
            entry(1, 1, 1000, t),
            entry(4, 1, 1000, t + Duration::from_secs(3)),
            entry(2, 2, 1250, t + Duration::from_secs(1)),
            entry(5, 5, 1600, t + Duration::from_secs(4)),
        ];
        let mut ranked = Ranked { queue, ..Ranked::default() };
        let pairs = ranked.pair_up(t + Duration::from_secs(4));
        let conns: Vec<[ConnId; 2]> = pairs.iter().map(|[a, b]| [a.conn, b.conn]).collect();
        assert_eq!(conns, [[1, 3]], "closest in ELO, never the same account");
        let left: Vec<ConnId> = ranked.queue.iter().map(|e| e.conn).collect();
        assert_eq!(left, [2, 4, 5], "the others wait on, oldest first");
    }
}
