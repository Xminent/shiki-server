use actix::Addr;
use actix_web::{get, post, web, HttpResponse, Responder};
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

/// Shows all the rooms available
#[get("/rooms")]
async fn get_rooms_list(
	srv: web::Data<Addr<crate::server::ShikiServer>>,
) -> HttpResponse {
	match srv.send(crate::server::ListChannels).await {
		Ok(rooms) => HttpResponse::Ok().json(rooms),
		Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
	}
}

/// Creates a new room.
#[post("/rooms")]
async fn create_room(
	data: web::Json<String>, srv: web::Data<Addr<crate::server::ShikiServer>>,
) -> HttpResponse {
	match srv
		.send(crate::server::ChannelCreate {
			id: 0,
			name: data.into_inner(),
			sessions: HashSet::new(),
		})
		.await
	{
		Ok(Some(room)) => HttpResponse::Ok().json(room),
		Ok(None) => HttpResponse::BadRequest().body("Room already exists"),
		Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
	}
}

/// Joins a room
#[post("/rooms/{room_id}/join")]
async fn join_room(
	room_id: web::Path<i64>, srv: web::Data<Addr<crate::server::ShikiServer>>,
) -> HttpResponse {
	match srv
		.send(crate::server::Join { client_id: 0, room_id: *room_id })
		.await
	{
		Ok(Some(room)) => HttpResponse::Ok().json(room),
		Ok(None) => HttpResponse::BadRequest().body("Room does not exist"),
		Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
	}
}

pub fn routes(cfg: &mut web::ServiceConfig) {
	cfg.service(
		web::scope("/api")
			.service(get_count)
			.service(get_rooms_list)
			.service(create_room)
			.service(join_room),
	);
}
