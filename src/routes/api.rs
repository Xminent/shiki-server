use super::middleware::Auth;
use crate::{
	models::{Channel, Message, User},
	routes::{CHANNEL_COLL_NAME, DB_NAME, MESSAGE_COLL_NAME, USER_COLL_NAME},
	ws::server::{self, CreateMessage, Join, ListChannels, ShikiServer},
};
use actix::Addr;
use actix_web::{get, patch, post, web, HttpResponse, Responder};
use futures::TryStreamExt;
use mongodb::{bson::doc, options::FindOptions, Client};
use serde::{Deserialize, Serialize};
use snowflake::SnowflakeIdGenerator;
use std::{
	collections::{HashMap, HashSet},
	sync::{
		atomic::{AtomicUsize, Ordering},
		Mutex,
	},
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
	let id = snowflake_gen.lock().unwrap().real_time_generate();
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
	data: web::Query<GetMessages>,
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
	let user_ids: HashSet<i64> =
		messages.iter().map(|msg| msg.author_id).collect();

	log::debug!("User IDs: {:?}", user_ids);

	// Fetch the users

	let cursor = client
		.database(DB_NAME)
		.collection::<User>(USER_COLL_NAME)
		.find(
			doc! {
				"id": {
					"$in": user_ids.iter().map(|id| *id).collect::<Vec<i64>>()
				}
			},
			None,
		)
		.await;

	let users = match cursor {
		Ok(cursor) => cursor.try_collect::<Vec<User>>().await,
		Err(_) => {
			return HttpResponse::InternalServerError()
				.body("Something went wrong");
		}
	};

	log::debug!("Users: {:?}", users);

	let users: HashMap<i64, server::User> = match users {
		Ok(users) => {
			users.into_iter().map(|user| (user.id, user.into())).collect()
		}
		Err(_) => HashMap::new(),
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

	data.author = server::User {
		id: user.id,
		username: user.username,
		joined: user.created_at,
		avatar: user.avatar,
	};

	let channel_id = channel_id.into_inner();
	let id = snowflake_gen.lock().unwrap().real_time_generate();
	let message = Message::new(id, channel_id, user.id, &data.content);

	data.channel_id = channel_id;

	let res = client
		.database(DB_NAME)
		.collection::<Message>(MESSAGE_COLL_NAME)
		.insert_one(message, None)
		.await;

	if res.is_err() {
		return HttpResponse::InternalServerError()
			.body("Something went wrong");
	}

	match srv.send(data).await {
		Ok(Some(msg)) => HttpResponse::Ok().json(msg),
		Ok(None) => HttpResponse::BadRequest().body("Channel does not exist!"),
		Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
	}
}

#[derive(Deserialize, Validate)]
struct ModifyUser {
	/// User's username
	#[validate(length(min = 3, max = 20))]
	pub username: Option<String>,
	/// User's avatar
	pub avatar: Option<String>,
}

/// Modify the requester's user account settings. Returns a user object on success.
// TODO: Fire a User Update Gateway event.
#[patch("/users/@me")]
async fn modify_user(
	client: web::Data<Client>, data: web::Json<ModifyUser>,
	_srv: web::Data<Addr<ShikiServer>>, mut user: User,
) -> HttpResponse {
	if let Err(err) = data.validate() {
		return HttpResponse::BadRequest().json(err);
	}

	let data = data.into_inner();

	if let Some(username) = data.username {
		user.username = username;
	}

	if let Some(avatar) = data.avatar {
		user.avatar = Some(avatar);
	}

	let res = client
		.database(DB_NAME)
		.collection::<User>(USER_COLL_NAME)
		.replace_one(doc! {"email": &user.email}, user.clone(), None)
		.await;

	match res {
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

pub fn routes(client: &Client, cfg: &mut web::ServiceConfig) {
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
