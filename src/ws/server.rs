use super::events::{self, Event};
use crate::{
	routes::{CHANNEL_COLL_NAME, DB_NAME},
	utils::{self},
	ws::events::Ready,
};
use actix::prelude::*;
use futures::stream::StreamExt;
use mongodb::{bson::doc, Client};
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
#[derive(Message, Serialize, Debug, Clone, Deserialize)]
#[rtype(result = "Option<Channel>")]
pub struct Channel {
	/// Channel ID
	pub id: i64,
	/// Guild ID
	pub guild_id: Option<i64>,
	/// Channel name
	pub name: String,
	/// IDs of sessions in the channel
	#[serde(skip_serializing, skip_deserializing)]
	pub sessions: HashSet<usize>,
}

/// Create new Guild
#[derive(Message, Serialize, Debug, Clone, Deserialize)]
#[rtype(result = "Option<Guild>")]
pub struct Guild {
	/// ID of the guild
	pub id: i64,
	/// Guild name
	pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct User {
	pub id: i64,
	pub username: String,
	pub joined: usize,
	/// Avatar URL
	pub avatar: Option<String>,
}

/// Create new message
#[derive(Message, Serialize, Deserialize, Debug, Clone)]
#[rtype(result = "Option<CreateMessage>")]
pub struct CreateMessage {
	/// Message ID
	#[serde(skip_deserializing)]
	pub id: i64,
	/// Channel ID
	#[serde(skip_deserializing)]
	pub channel_id: i64,
	/// Message content
	pub content: String,
	/// User who sent the message
	#[serde(skip_deserializing)]
	pub author: User,
}

/// List of available channels
#[derive(Message)]
#[rtype(result = "Vec<Channel>")]
pub struct ListChannels;

/// Join channel, if channel does not exists create new one.
#[derive(Message)]
#[rtype(result = "Option<Channel>")]
pub struct Join {
	/// Client ID
	pub client_id: usize,
	/// Channel ID
	pub channel_id: i64,
}

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
	channels: HashMap<i64, Channel>,
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
	) -> Self {
		Self {
			client,
			sessions: HashMap::new(),
			channels: HashMap::new(),
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
	fn send_to_everyone(&self, message: Event, skip_id: usize) {
		// message every session.
		for (id, addr) in &self.sessions {
			if *id != skip_id {
				addr.do_send(message.clone());
			}
		}
	}
}

/// Make actor from `ChatServer`
impl Actor for ShikiServer {
	/// We are going to use simple Context, we just need ability to communicate
	/// with other actors.
	type Context = Context<Self>;

	fn started(&mut self, ctx: &mut Self::Context) {
		let client_clone = self.client.clone();

		async move {
			let mut channels = HashMap::<i64, Channel>::new();
			let cursor = client_clone
				.database(DB_NAME)
				.collection::<Channel>(CHANNEL_COLL_NAME)
				.find(None, None)
				.await;

			if let Err(e) = cursor {
				log::error!("Failed to load channels: {}", e);
				return None;
			}

			let mut cursor = cursor.unwrap();

			while let Some(channel) = cursor.next().await {
				if let Ok(channel) = channel {
					log::debug!("Loaded channel: {:?}", channel);
					channels.insert(channel.id, channel.clone());
				}
			}

			Some(channels)
		}
		.into_actor(self)
		.then(move |res, act, _| {
			if let Some(channels) = res {
				act.channels = channels;
			}

			fut::ready(())
		})
		.wait(ctx);

		log::info!("Chat server started");
		log::info!("Loaded {} channels", self.channels.len());
	}
}

impl Handler<Connect> for ShikiServer {
	type Result = usize;

	fn handle(&mut self, msg: Connect, _: &mut Context<Self>) -> Self::Result {
		log::debug!("Someone joined");

		// register session with random id
		let id = self.rng.gen::<usize>();
		self.sessions.insert(id, msg.addr.clone());

		// Insert the user into every single channel's sessions.
		for channel in self.channels.values_mut() {
			channel.sessions.insert(id);
		}

		// Send an Identify event to the client so they may authenticate themselves.
		msg.addr.do_send(Event::Hello);

		let count = self.visitor_count.fetch_add(1, Ordering::SeqCst);

		log::info!("{} visitors online", count);

		id
	}
}

impl Handler<Disconnect> for ShikiServer {
	type Result = ();

	fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
		log::info!("{} disconnected", msg.id);

		let count = self.visitor_count.fetch_update(
			Ordering::SeqCst,
			Ordering::SeqCst,
			|current| {
				if current > 0 {
					Some(current - 1)
				} else {
					None
				}
			},
		);

		if let Ok(updated_count) = count {
			log::info!("{} visitors online", updated_count);
		}

		let mut channels: Vec<i64> = Vec::new();

		if self.sessions.remove(&msg.id).is_some() {
			for channel in self.channels.values_mut() {
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

		let channels = self.channels.clone();
		let client_clone = self.client.clone();

		async move {
			let res =
				utils::validate_token(client_clone.clone(), msg.token.clone())
					.await;

			if res.is_none() {
				log::warn!("Invalid token");
				return session.do_send(Event::BadToken);
			}

			let user = res.unwrap();

			session.do_send(Event::SetToken(msg.token));

			log::info!(
				"User {} authenticated, sending Ready payload...",
				user.username
			);

			let users = utils::get_all_users(client_clone)
				.await
				.into_iter()
				.map(|u| User {
					username: u.username,
					id: u.id,
					avatar: u.avatar,
					joined: u.created_at,
				})
				.collect();

			session.do_send(Event::Ready(Ready {
				channels: channels.values().cloned().collect(),
				user: User {
					username: user.username,
					id: user.id,
					avatar: user.avatar,
					joined: user.created_at,
				},
				users,
			}));
		}
		.into_actor(self)
		.then(|_, _, _| fut::ready(()))
		.wait(ctx);
	}
}

impl Handler<Channel> for ShikiServer {
	type Result = MessageResult<Channel>;

	fn handle(&mut self, msg: Channel, _: &mut Context<Self>) -> Self::Result {
		log::info!("Channel created");

		if self.channels.contains_key(&msg.id) {
			return MessageResult(None);
		}

		self.channels.insert(msg.id, msg.clone());

		self.send_to_everyone(
			Event::ChannelCreate(events::ChannelCreate::new(
				msg.id,
				msg.name.clone(),
			)),
			0,
		);

		MessageResult(Some(msg))
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
			msg.channel_id,
			msg.author.clone(),
		);

		self.send_channel_message(channel.id, Event::MessageCreate(event), 0);

		MessageResult(Some(CreateMessage {
			id,
			channel_id: msg.channel_id,
			content: msg.content,
			author: msg.author,
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
