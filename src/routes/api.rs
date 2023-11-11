use super::middleware::Auth;
use crate::{
	models::User,
	ws::server::{
		self, CreateChannel, CreateMessage, Join, ListChannels, ShikiServer,
	},
};
use actix::Addr;
use actix_web::{get, post, web, HttpResponse, Responder};
use mongodb::Client;
use std::{
	collections::HashSet,
	sync::atomic::{AtomicUsize, Ordering},
};

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
		avatar: None,
	};

	data.channel_id = channel_id.into_inner();

	log::debug!("Data: {:?}", data);

	match srv.send(data).await {
		Ok(Some(msg)) => HttpResponse::Ok().json(msg),
		Ok(None) => HttpResponse::BadRequest().body("Channel does not exist!"),
		Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
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
			.wrap(Auth::new(client.clone())),
	);
}
