use crate::{
    events::Ready,
    utils::{self},
};
use actix::prelude::*;
use mongodb::Client;
use rand::{self, rngs::ThreadRng, Rng};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

/// Chat server sends this messages to session
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub enum Event {
    Ready(Ready),
    Custom(String),
}

/// Message for chat server communications

/// New chat session is created
#[derive(Message)]
#[rtype(usize)]
pub struct Connect {
    pub addr: Recipient<Event>,
}

/// Session is disconnected
#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect {
    pub id: usize,
}

/// Payload sent from client to identify itself.
#[derive(Message)]
#[rtype(result = "()")]
pub struct Identify {
    pub id: usize,
    pub token: String,
}

/// Send message to specific room
#[derive(Message)]
#[rtype(result = "()")]
pub struct ClientMessage {
    /// Id of the client session
    pub id: usize,
    /// Peer message
    pub msg: String,
    /// Room name
    pub room: String,
}

/// List of available rooms
pub struct ListRooms;

impl actix::Message for ListRooms {
    type Result = Vec<String>;
}

/// Join room, if room does not exists create new one.
#[derive(Message)]
#[rtype(result = "()")]
pub struct Join {
    /// Client ID
    pub id: usize,
    /// Room name
    pub name: String,
}

/// `ShikiServer` manages chat rooms and responsible for coordinating chat session.
///
/// Implementation is very naïve.
#[derive(Debug)]
pub struct ShikiServer {
    /// MongoDB client
    client: Client,
    /// The actual connected clients to the gateway.
    sessions: HashMap<usize, Recipient<Event>>,
    /// Chat rooms. In this case they're individual rooms where messages are propagated to users in the same room. This could be a channel, guild, etc.
    rooms: HashMap<String, HashSet<usize>>,
    /// Random generator for making unique IDs.
    rng: ThreadRng,
    /// Number of connected clients
    visitor_count: Arc<AtomicUsize>,
}

impl ShikiServer {
    pub fn new(client: Client, visitor_count: Arc<AtomicUsize>) -> ShikiServer {
        // default room
        let mut rooms = HashMap::new();
        rooms.insert("main".to_owned(), HashSet::new());

        ShikiServer {
            client,
            sessions: HashMap::new(),
            rooms,
            rng: rand::thread_rng(),
            visitor_count,
        }
    }
}

impl ShikiServer {
    /// Send message to all users in the room
    fn send_room_message(&self, room: &str, message: &str, skip_id: usize) {
        if let Some(sessions) = self.rooms.get(room) {
            log::debug!("Sending message to {} sessions", sessions.len());

            for id in sessions {
                if *id != skip_id {
                    if let Some(addr) = self.sessions.get(id) {
                        addr.do_send(Event::Custom(message.to_owned()));
                    }
                }
            }
        }
    }
}

/// Make actor from `ChatServer`
impl Actor for ShikiServer {
    /// We are going to use simple Context, we just need ability to communicate
    /// with other actors.
    type Context = Context<Self>;
}

/// Handler for Connect message.
///
/// Register new session and assign unique id to this session
impl Handler<Connect> for ShikiServer {
    type Result = usize;

    fn handle(&mut self, msg: Connect, _: &mut Context<Self>) -> Self::Result {
        log::debug!("Someone joined");

        // register session with random id
        let id = self.rng.gen::<usize>();
        self.sessions.insert(id, msg.addr.clone());
        self.rooms.entry("main".to_owned()).or_default().insert(id);

        // Send an Identify event to the client so they may authenticate themselves.
        msg.addr.do_send(Event::Custom(id.to_string()));

        let count = self.visitor_count.fetch_add(1, Ordering::SeqCst);

        log::info!("{} visitors online", count);

        id
    }
}

/// Handler for Disconnect message.
impl Handler<Disconnect> for ShikiServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
        log::info!("Someone disconnected");

        let mut rooms: Vec<String> = Vec::new();

        // remove address
        if self.sessions.remove(&msg.id).is_some() {
            // remove session from all rooms
            for (name, sessions) in &mut self.rooms {
                if sessions.remove(&msg.id) {
                    rooms.push(name.to_owned());
                }
            }
        }

        // send message to other users
        for room in rooms {
            self.send_room_message(&room, "Someone disconnected", 0);
        }
    }
}

/// Handler for Identify message.
impl Handler<Identify> for ShikiServer {
    type Result = ();

    fn handle(&mut self, msg: Identify, ctx: &mut Context<Self>) {
        let session = if let Some(s) = self.sessions.get(&msg.id).cloned() {
            s
        } else {
            return;
        };

        // Check if the passed token is valid, if not send disconnect message.
        utils::validate_token(self.client.clone(), msg.token.clone())
            .into_actor(self)
            .then(move |res, _, _| {
                if let Some(user) = res {
                    log::info!(
                        "User {} authenticated, sending Ready payload...",
                        user.username
                    );

                    session.do_send(Event::Ready(Ready {
                        id: user.id,
                        name: user.username,
                    }))
                } else {
                    log::info!("Invalid token");
                    session.do_send(Event::Custom("Invalid token".to_owned()));
                }

                fut::ready(())
            })
            .wait(ctx);
    }
}

/// Handler for Message message.
impl Handler<ClientMessage> for ShikiServer {
    type Result = ();

    fn handle(&mut self, msg: ClientMessage, _: &mut Context<Self>) {
        self.send_room_message(&msg.room, &msg.msg, msg.id);
    }
}

/// Handler for `ListRooms` message.
impl Handler<ListRooms> for ShikiServer {
    type Result = MessageResult<ListRooms>;

    fn handle(&mut self, _: ListRooms, _: &mut Context<Self>) -> Self::Result {
        MessageResult(self.rooms.keys().cloned().collect())
    }
}

/// Join room, send disconnect message to old room
/// send join message to new room
impl Handler<Join> for ShikiServer {
    type Result = ();

    fn handle(&mut self, msg: Join, _: &mut Context<Self>) {
        let Join { id, name } = msg;
        let mut rooms = Vec::new();

        // remove session from all rooms
        for (n, sessions) in &mut self.rooms {
            if sessions.remove(&id) {
                rooms.push(n.to_owned());
            }
        }

        // send message to other users
        for room in rooms {
            self.send_room_message(&room, "Someone disconnected", 0);
        }

        self.rooms.entry(name.clone()).or_default().insert(id);
        self.send_room_message(&name, "Someone connected", id);
    }
}
