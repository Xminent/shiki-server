use super::middleware::Auth;
use crate::{
	models::User,
	routes::{DB_NAME, USER_COLL_NAME},
	ws::server::{
		self, CreateChannel, CreateMessage, Join, ListChannels, ShikiServer,
	},
};
use actix::Addr;
use actix_web::{get, patch, post, web, HttpResponse, Responder};
use mongodb::{bson::doc, Client};
use serde::Deserialize;
use std::{
	collections::HashSet,
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

/// Creates a new channel.
#[post("/channels")]
async fn create_channel(
	data: web::Json<String>, srv: web::Data<Addr<ShikiServer>>,
) -> HttpResponse {
	match srv
		.send(CreateChannel {
			id: 0,
			name: data.into_inner(),
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

/// Creates a new message
#[post("/channels/{channel_id}/messages")]
async fn create_message(
	channel_id: web::Path<i64>, data: web::Json<CreateMessage>,
	srv: web::Data<Addr<ShikiServer>>, user: User,
) -> HttpResponse {
	let mut data = data.into_inner();

	data.author = server::User {
		id: user.id,
		username: user.username,
		joined: user.created_at,
		avatar: user.avatar,
	};

	data.channel_id = channel_id.into_inner();

	log::debug!("Data: {:?}", data);

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
			.service(modify_user)
			.wrap(Auth::new(client.clone())),
	);
}
