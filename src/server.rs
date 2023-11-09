use crate::{
	events::{self, HasOpcode, MessageCreate, Opcode, Ready},
	utils::{self},
};
use actix::prelude::*;
use mongodb::Client;
use rand::{self, rngs::ThreadRng, Rng};
use serde::{Deserialize, Serialize};
use snowflake::SnowflakeIdGenerator;
use std::{
	collections::{HashMap, HashSet},
	sync::{
		atomic::{AtomicUsize, Ordering},
		Arc, Mutex,
	},
};

/// Chat server sends this messages to session
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub enum Event {
	Ready(Ready),
	MessageCreate(MessageCreate),
	Custom(String),
}

impl Event {
	pub fn opcode(&self) -> Opcode {
		match self {
			Event::Ready(_) => Ready::opcode(),
			Event::MessageCreate(_) => MessageCreate::opcode(),
			Event::Custom(_) => Opcode::Custom,
		}
	}
}

impl Serialize for Event {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		match self {
			Event::Ready(ready) => ready.serialize(serializer),
			Event::MessageCreate(message) => message.serialize(serializer),
			Event::Custom(msg) => serializer.serialize_str(msg),
		}
	}
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

/// Create new channel
#[derive(Serialize, Debug, Clone)]
pub struct ChannelCreate {
	/// Channel ID
	pub id: i64,
	/// Channel name
	pub name: String,
	/// IDs of sessions in the channel
	// #[serde(skip_serializing)]
	pub sessions: HashSet<usize>,
}

impl actix::Message for ChannelCreate {
	type Result = Option<ChannelCreate>;
}

/// Create new message
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CreateMessage {
	/// Message ID
	#[serde(skip_deserializing)]
	pub id: i64,
	/// Channel ID
	#[serde(skip_deserializing)]
	pub channel_id: i64,
	/// Message content
	pub content: String,
}

impl actix::Message for CreateMessage {
	type Result = Option<CreateMessage>;
}

/// List of available channels
pub struct ListChannels;

impl actix::Message for ListChannels {
	type Result = Vec<ChannelCreate>;
}

/// Join channel, if channel does not exists create new one.
pub struct Join {
	/// Client ID
	pub client_id: usize,
	/// Channel ID
	pub channel_id: i64,
}

impl actix::Message for Join {
	type Result = Option<ChannelCreate>;
}

const DEFAULT_CHANNEL: i64 = 0;

/// `ShikiServer` manages chat channels and responsible for coordinating chat session.
///
/// Implementation is very naïve.
#[derive(Debug)]
pub struct ShikiServer {
	/// MongoDB client
	client: Client,
	/// The actual connected clients to the gateway.
	sessions: HashMap<usize, Recipient<Event>>,
	/// Chat channels. In this case they're individual channels where messages are propagated to users in the same channel. This could be a channel, guild, etc.
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
		&self, channel: i64, message: Event, skip_id: usize,
	) {
		if let Some(sessions) =
			self.channels.get(&channel).map(|channel| &channel.sessions)
		{
			log::debug!(
				"Sending message to {} sessions in channel {channel}",
				sessions.len()
			);

			for id in sessions {
				if *id != skip_id {
					if let Some(addr) = self.sessions.get(id) {
						addr.do_send(message.clone());
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

impl Handler<Disconnect> for ShikiServer {
	type Result = ();

	fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
		log::info!("{} disconnected", msg.id);

		let mut channels: Vec<i64> = Vec::new();

		if self.sessions.remove(&msg.id).is_some() {
			for (_, channel) in &mut self.channels {
				if channel.sessions.remove(&msg.id) {
					channels.push(channel.id);
				}
			}
		}

		for channel in channels {
			self.send_channel_message(
				channel,
				Event::Custom(format!("{} left", msg.id)),
				0,
			);
		}
	}
}

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

impl Handler<ChannelCreate> for ShikiServer {
	type Result = MessageResult<ChannelCreate>;

	fn handle(
		&mut self, msg: ChannelCreate, _: &mut Context<Self>,
	) -> Self::Result {
		log::info!("Channel created");

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

impl Handler<CreateMessage> for ShikiServer {
	type Result = MessageResult<CreateMessage>;

	fn handle(
		&mut self, msg: CreateMessage, _: &mut Context<Self>,
	) -> Self::Result {
		log::info!("Message created");

		let id = self.snowflake_gen.lock().unwrap().real_time_generate();

		let channel = self.channels.get(&msg.channel_id);

		if channel.is_none() {
			return MessageResult(None);
		}

		let channel = channel.unwrap();

		let event = events::MessageCreate::new(
			id,
			msg.content.clone(),
			None,
			msg.channel_id,
		);

		self.send_channel_message(channel.id, Event::MessageCreate(event), 0);

		MessageResult(Some(CreateMessage {
			id,
			channel_id: msg.channel_id,
			content: msg.content,
		}))
	}
}

impl Handler<ListChannels> for ShikiServer {
	type Result = MessageResult<ListChannels>;

	fn handle(
		&mut self, _: ListChannels, _: &mut Context<Self>,
	) -> Self::Result {
		MessageResult(self.channels.values().cloned().collect())
	}
}

impl Handler<Join> for ShikiServer {
	type Result = MessageResult<Join>;

	fn handle(&mut self, msg: Join, _: &mut Context<Self>) -> Self::Result {
		let Join { client_id, channel_id } = msg;

		match self.channels.get_mut(&channel_id) {
			Some(channel) => {
				channel.sessions.insert(client_id);
			}

			None => {
				return MessageResult(None);
			}
		}

		self.send_channel_message(
			channel_id,
			Event::Custom("Someone connected".to_owned()),
			client_id,
		);

		MessageResult(Some(self.channels.get_mut(&channel_id).unwrap().clone()))
	}
}
