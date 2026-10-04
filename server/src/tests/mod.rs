use futures_util::{SinkExt, StreamExt};
use shared::{
    config, ClientMessage, GameState, IncomingGarbage, InputKind, LobbyInfo, PausePolicy, PuyoType, RoomId,
    ServerMessage,
};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

use super::*;
use crate::manager::friends::*;
use crate::manager::ranked::*;
use crate::manager::social::*;
use crate::manager::*;
use crate::room::*;
use crate::sim::*;
use crate::ws::*;

mod connection;
mod friends;
mod game;
mod ranked;
mod rooms;
mod shutdown;
mod social;

use friends::*;
use game::*;
use ranked::*;
use rooms::*;
use shutdown::*;

fn new_mgr() -> Manager {
    Manager::new()
}

fn reg(mgr: &mut Manager, conn: ConnId) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel(CLIENT_CHAN_CAP);
    mgr.handle(Command::Register { conn, sender: tx });
    rx
}

fn hello(mgr: &mut Manager, conn: ConnId, token: &str) {
    mgr.handle(Command::Hello {
        last_disconnect_reason: None,
        conn,
        token: token.to_string(),
        user_id: None,
        username: None,
        session: None,
    });
}

fn drain(rx: &mut mpsc::Receiver<Vec<u8>>) -> Vec<ServerMessage> {
    let mut out = Vec::new();
    while let Ok(bytes) = rx.try_recv() {
        if let Some(msg) = shared::decode::<ServerMessage>(&bytes) {
            out.push(msg);
        }
    }
    out
}

fn last_lobby(msgs: &[ServerMessage]) -> Option<LobbyInfo> {
    msgs.iter().rev().find_map(|m| match m {
        ServerMessage::Lobby { info } => Some(info.clone()),
        _ => None,
    })
}

fn has(msgs: &[ServerMessage], f: impl Fn(&ServerMessage) -> bool) -> bool {
    msgs.iter().any(f)
}

fn two_player_room(mgr: &mut Manager) -> (RoomId, mpsc::Receiver<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    let rx1 = reg(mgr, 1);
    hello(mgr, 1, "A");
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "R".into(),
    });
    let rx2 = reg(mgr, 2);
    hello(mgr, 2, "B");
    mgr.handle(Command::JoinRoom { conn: 2, id: 1 });
    (1, rx1, rx2)
}

#[test]
fn create_room_lobbies_host() {
    let mut mgr = new_mgr();
    let mut rx1 = reg(&mut mgr, 1);
    hello(&mut mgr, 1, "A");
    mgr.handle(Command::CreateRoom {
        conn: 1,
        name: "Room".into(),
    });
    let info = last_lobby(&drain(&mut rx1)).expect("host should get a Lobby");
    assert_eq!(info.players, 1);
    assert_eq!(info.your_slot, 1);
    assert!(info.is_host);
    assert!(mgr.rooms.contains_key(&1));
}
