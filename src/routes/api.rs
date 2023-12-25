use super::middleware::Auth;
use crate::{
	models::{Channel, Message, User},
	redis::{ModifyUser, RedisFetcher},
	routes::{CHANNEL_COLL_NAME, DB_NAME, MESSAGE_COLL_NAME},
	ws::server::{self, CreateMessage, Join, ListChannels, ShikiServer},
};
use actix::Addr;
use actix_web::{get, patch, post, web, HttpResponse, Responder};
use futures::TryStreamExt;
use futures_util::lock::Mutex;
use mongodb::{bson::doc, options::FindOptions, Client};
use serde::{Deserialize, Serialize};
use snowflake::SnowflakeIdGenerator;
use std::{
	collections::{HashMap, HashSet},
	sync::atomic::{AtomicUsize, Ordering},
};
use validator::Validate;

/// Displays state
#[get("/count")]
async fn get_count(count: web::Data<AtomicUsize>) -> impl Responder {
	let current_count = count.load(Ordering::SeqCst);
	format!("Visitors: {current_count}")
}

/// Shows all the channels available
#[get("/channels")]
async fn get_channels_list(
	srv: web::Data<Addr<crate::ws::server::ShikiServer>>,
) -> HttpResponse {
	match srv.send(ListChannels).await {
		Ok(channels) => HttpResponse::Ok().json(channels),
		Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
	}
}

#[derive(Deserialize, Validate, Serialize)]
struct CreateChannel {
	#[validate(length(min = 1))]
	pub name: String,
}

/// Creates a new channel.
#[post("/channels")]
async fn create_channel(
	client: web::Data<Client>, data: web::Json<CreateChannel>,
	snowflake_gen: web::Data<Mutex<SnowflakeIdGenerator>>,
	srv: web::Data<Addr<ShikiServer>>, user: User,
) -> HttpResponse {
	if let Err(err) = data.validate() {
		return HttpResponse::BadRequest().json(err);
	}

	let data = data.into_inner();
	let id = snowflake_gen.lock().await.real_time_generate();
	let channel = Channel::new(id, &data.name, None, user.id);

	let res = client
		.database(DB_NAME)
		.collection::<Channel>(CHANNEL_COLL_NAME)
		.insert_one(channel, None)
		.await;

	if res.is_err() {
		return HttpResponse::InternalServerError()
			.body("Something went wrong");
	}

	match srv
		.send(server::Channel {
			id,
			guild_id: None,
			name: data.name,
			sessions: HashSet::new(),
		})
		.await
	{
		Ok(Some(channel)) => HttpResponse::Ok().json(channel),
		Ok(None) => HttpResponse::BadRequest().body("Channel already exists"),
		Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
	}
}

/// Joins a channel
// NOTE: This is should be an internal feature, caused by the future addition of channel viewing permissions. Editing said permissions should allow a user to effectively "join" a channel.
#[post("/channels/{channel_id}/join")]
async fn join_channel(
	channel_id: web::Path<i64>, srv: web::Data<Addr<ShikiServer>>,
) -> HttpResponse {
	match srv.send(Join { client_id: 0, channel_id: *channel_id }).await {
		Ok(Some(channel)) => HttpResponse::Ok().json(channel),
		Ok(None) => HttpResponse::BadRequest().body("Channel does not exist"),
		Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
	}
}

#[derive(Deserialize)]
struct GetMessages {
	/// Get messages before this message ID
	#[serde(default = "default_before")]
	before: Option<i64>,
	/// Get messages after this message ID
	#[serde(default = "default_after")]
	after: Option<i64>,
	/// Max number of messages to return (1-100)
	#[serde(default = "default_limit")]
	limit: i64,
}

fn default_before() -> Option<i64> {
	None
}

fn default_after() -> Option<i64> {
	None
}

fn default_limit() -> i64 {
	50
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct GetMessage {
	/// The id of the message
	pub id: i64,
	/// The id of the channel the message was sent in
	pub channel_id: i64,
	/// The content of the message
	pub content: String,
	/// Unix timestamp for when the message was created
	pub created_at: usize,
	/// User who sent the message
	pub author: server::User,
}

/// Fetches the messages in a channel
#[get("/channels/{channel_id}/messages")]
async fn get_messages(
	channel_id: web::Path<i64>, client: web::Data<Client>,
	data: web::Query<GetMessages>, fetcher: web::Data<RedisFetcher>,
) -> HttpResponse {
	if data.limit < 1 || data.limit > 100 {
		return HttpResponse::BadRequest()
			.body("Limit must be between 1 and 100");
	}

	let mut query = doc! {
		"channel_id": *channel_id
	};

	if let Some(before) = data.before {
		query.insert(
			"id",
			doc! {
				"$lt": before
			},
		);
	}

	if let Some(after) = data.after {
		query.insert(
			"id",
			doc! {
				"$gt": after
			},
		);
	}

	let cursor = client
		.database(DB_NAME)
		.collection::<Message>(MESSAGE_COLL_NAME)
		.find(
			query,
			Some(
				FindOptions::builder()
					.sort(doc! {"id": 1})
					.limit(data.limit)
					.build(),
			),
		)
		.await;

	let messages = match cursor {
		Ok(cursor) => match cursor.try_collect::<Vec<Message>>().await {
			Ok(res) => res,
			Err(_) => {
				return HttpResponse::InternalServerError()
					.body("Something went wrong");
			}
		},

		Err(_) => {
			return HttpResponse::InternalServerError()
				.body("Something went wrong");
		}
	};

	// Make a set of all of the user IDs mentioned in the messages.
	let user_ids = messages
		.iter()
		.map(|msg| msg.author_id)
		.collect::<HashSet<i64>>()
		.into_iter()
		.collect::<Vec<i64>>();

	log::debug!("User IDs: {:?}", user_ids);

	// Fetch the users
	let users = fetcher.fetch_users(Some(&user_ids)).await;
	let users: HashMap<i64, server::User> = match users {
		Ok(users) => {
			if users.len() < user_ids.len() {
				log::error!(
					"Missing {:?} users when fetching messages",
					user_ids.len() - users.len()
				);

				return HttpResponse::InternalServerError()
					.body("Something went wrong");
			}

			users.into_iter().map(|user| (user.id, user.into())).collect()
		}
		Err(_) => {
			return HttpResponse::InternalServerError()
				.body("Something went wrong");
		}
	};

	let messages: Vec<GetMessage> = messages
		.into_iter()
		.map(|msg| GetMessage {
			id: msg.id,
			channel_id: msg.channel_id,
			content: msg.content,
			created_at: msg.created_at,
			author: users.get(&msg.author_id).cloned().unwrap_or_default(),
		})
		.collect();

	HttpResponse::Ok().json(messages)
}

/// Creates a new message
#[post("/channels/{channel_id}/messages")]
async fn create_message(
	channel_id: web::Path<i64>, client: web::Data<Client>,
	data: web::Json<CreateMessage>,
	snowflake_gen: web::Data<Mutex<SnowflakeIdGenerator>>,
	srv: web::Data<Addr<ShikiServer>>, user: User,
) -> HttpResponse {
	let mut data = data.into_inner();

	data.id = snowflake_gen.lock().await.real_time_generate();
	data.channel_id = channel_id.into_inner();
	data.author = server::User {
		id: user.id,
		username: user.username,
		joined: user.created_at,
		avatar: user.avatar,
	};

	let res = client
		.database(DB_NAME)
		.collection::<Message>(MESSAGE_COLL_NAME)
		.insert_one(Message::from(data.clone()), None)
		.await;

	if res.is_err() {
		return HttpResponse::InternalServerError()
			.body("Something went wrong");
	}

	// TODO: Refactor this so the response is not dependent on the gateway's response. Messages should still return 200s even if the gateway were to be down.
	match srv.send(data).await {
		Ok(Some(msg)) => HttpResponse::Ok().json(msg),
		Ok(None) => HttpResponse::BadRequest().body("Channel does not exist!"),
		Err(err) => {
			log::error!("Failed to send message: {:?}", err);

			HttpResponse::InternalServerError().body("Something went wrong")
		}
	}
}

/// Modify the requester's user account settings. Returns a user object on success.
// TODO: Fire a User Update Gateway event.
#[patch("/users/@me")]
async fn modify_user(
	data: web::Json<ModifyUser>, fetcher: web::Data<RedisFetcher>,
	mut user: User,
) -> HttpResponse {
	if let Err(err) = data.validate() {
		return HttpResponse::BadRequest().json(err);
	}

	match fetcher.modify_user(&mut user, data.into_inner()).await {
		Ok(_) => HttpResponse::Ok().json(server::User {
			id: user.id,
			username: user.username,
			joined: user.created_at,
			avatar: user.avatar,
		}),
		Err(err) => {
			log::error!("{:?}", err);
			HttpResponse::InternalServerError().body("Something went wrong")
		}
	}
}

pub fn routes(client: &RedisFetcher, cfg: &mut web::ServiceConfig) {
	cfg.service(
		web::scope("/api")
			.service(get_count)
			.service(get_channels_list)
			.service(create_channel)
			.service(join_channel)
			.service(create_message)
			.service(get_messages)
			.service(modify_user)
			.wrap(Auth::new(client.clone())),
	);
}
