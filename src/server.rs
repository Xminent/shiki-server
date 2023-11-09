use crate::{
	events::{self, Ready},
	utils::{self},
};
use actix::prelude::*;
use mongodb::Client;
use rand::{self, rngs::ThreadRng, Rng};
use serde::Serialize;
use snowflake::SnowflakeIdGenerator;
use std::{
	collections::{HashMap, HashSet},
	sync::{
		atomic::{AtomicUsize, Ordering},
		Arc, Mutex,
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

/// Create new room
#[derive(Serialize, Debug, Clone)]
pub struct ChannelCreate {
	/// Room ID
	pub id: i64,
	/// Room name
	pub name: String,
	/// IDs of sessions in the room
	// #[serde(skip_serializing)]
	pub sessions: HashSet<usize>,
}

impl actix::Message for ChannelCreate {
	type Result = Option<ChannelCreate>;
}

/// List of available rooms
pub struct ListChannels;

impl actix::Message for ListChannels {
	type Result = Vec<ChannelCreate>;
}

/// Join room, if room does not exists create new one.
pub struct Join {
	/// Client ID
	pub client_id: usize,
	/// Room ID
	pub room_id: i64,
}

impl actix::Message for Join {
	type Result = Option<ChannelCreate>;
}

const DEFAULT_CHANNEL: i64 = 0;

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
	channels: HashMap<i64, ChannelCreate>,
	/// Random generator for making unique IDs.
	rng: ThreadRng,
	/// Snowflake generator.
	snowflake_gen: Arc<Mutex<SnowflakeIdGenerator>>,
	/// Number of connected clients
	visitor_count: Arc<AtomicUsize>,
}

impl ShikiServer {
	pub fn new(
		client: Client, snowflake_gen: Arc<Mutex<SnowflakeIdGenerator>>,
		visitor_count: Arc<AtomicUsize>,
	) -> ShikiServer {
		let mut channels = HashMap::new();

		channels.insert(
			DEFAULT_CHANNEL,
			ChannelCreate {
				id: DEFAULT_CHANNEL,
				name: "main".to_owned(),
				sessions: HashSet::new(),
			},
		);

		ShikiServer {
			client,
			sessions: HashMap::new(),
			channels,
			rng: rand::thread_rng(),
			snowflake_gen,
			visitor_count,
		}
	}
}

impl ShikiServer {
	/// Send message to all users in the channel
	fn send_channel_message(
		&self, channel: i64, message: &str, skip_id: usize,
	) {
		if let Some(sessions) =
			self.channels.get(&channel).map(|room| &room.sessions)
		{
			log::debug!(
				"Sending message to {} sessions in channel {channel}",
				sessions.len()
			);

			for id in sessions {
				if *id != skip_id {
					if let Some(addr) = self.sessions.get(id) {
						addr.do_send(Event::Custom(message.to_owned()));
					}
				}
			}
		}
	}

	/// Send message to literally everyone.
	fn send_to_everyone(&self, message: &str, skip_id: usize) {
		// message every session.
		for (id, addr) in &self.sessions {
			if *id != skip_id {
				addr.do_send(Event::Custom(message.to_owned()));
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
		self.channels.get_mut(&DEFAULT_CHANNEL).unwrap().sessions.insert(id);

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
		log::info!("{} disconnected", msg.id);

		let mut rooms: Vec<i64> = Vec::new();

		if self.sessions.remove(&msg.id).is_some() {
			for (_, room) in &mut self.channels {
				if room.sessions.remove(&msg.id) {
					rooms.push(room.id);
				}
			}
		}

		for room in rooms {
			self.send_channel_message(room, &format!("{} left", msg.id), 0);
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

					session.do_send(Event::Ready(Ready::new(
						user.id,
						user.username,
					)));
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
impl Handler<events::MessageCreate> for ShikiServer {
	type Result = ();

	fn handle(
		&mut self, mut msg: events::MessageCreate, _: &mut Context<Self>,
	) {
		msg.id = self.snowflake_gen.lock().unwrap().real_time_generate();

		let msg_str = if let Ok(msg) = serde_json::to_string(&msg) {
			msg
		} else {
			return;
		};

		self.send_channel_message(msg.channel_id, &msg_str, 0);
	}
}

/// Handler for CreateRoom message.
impl Handler<ChannelCreate> for ShikiServer {
	type Result = MessageResult<ChannelCreate>;

	fn handle(
		&mut self, msg: ChannelCreate, _: &mut Context<Self>,
	) -> Self::Result {
		log::info!("Room created");

		let id = self.snowflake_gen.lock().unwrap().real_time_generate();
		let exists = self.channels.insert(
			id,
			ChannelCreate {
				id,
				name: msg.name.clone(),
				sessions: HashSet::new(),
			},
		);

		let channel: Self::Result = MessageResult(if exists.is_some() {
			None
		} else {
			Some(self.channels.get(&id).unwrap().clone())
		});

		if channel.0.is_some() {
			let msg_str = serde_json::to_string(&events::ChannelCreate::new(
				id, msg.name,
			));

			if let Ok(msg_str) = msg_str {
				self.send_to_everyone(&msg_str, 0);
			}
		}

		channel
	}
}

/// Handler for `ListRooms` message.
impl Handler<ListChannels> for ShikiServer {
	type Result = MessageResult<ListChannels>;

	fn handle(
		&mut self, _: ListChannels, _: &mut Context<Self>,
	) -> Self::Result {
		MessageResult(self.channels.values().cloned().collect())
	}
}

/// Join room, send disconnect message to old room
/// send join message to new room
impl Handler<Join> for ShikiServer {
	type Result = MessageResult<Join>;

	fn handle(&mut self, msg: Join, _: &mut Context<Self>) -> Self::Result {
		let Join { client_id, room_id } = msg;

		match self.channels.get_mut(&room_id) {
			Some(room) => {
				room.sessions.insert(client_id);
			}

			None => {
				return MessageResult(None);
			}
		}

		self.send_channel_message(room_id, "Someone connected", client_id);

		MessageResult(Some(self.channels.get_mut(&room_id).unwrap().clone()))
	}
}
