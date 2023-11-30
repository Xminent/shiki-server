use crate::ws::server::ShikiServer;
use actix::*;
use actix_cors::Cors;
use actix_files::Files;
use actix_web::{
	error, http,
	middleware::Logger,
	web::{self},
	App, HttpResponse, HttpServer,
};
use dotenv::dotenv;
use futures_util::{future, lock::Mutex};
use mongodb::{
	options::{ClientOptions, ResolverConfig},
	Client,
};
use snowflake::SnowflakeIdGenerator;
use std::{
	env,
	net::SocketAddr,
	sync::{atomic::AtomicUsize, Arc},
	time::{Duration, UNIX_EPOCH},
};
use webrtc_unreliable::Server;

mod auth;
mod errors;
mod models;
mod opus;
mod opusfile;
mod routes;
mod utils;
mod validator;
mod ws;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
	dotenv().expect("Failed to read .env file");

	env_logger::init_from_env(
		env_logger::Env::new().default_filter_or("debug"),
	);

	let db = Client::with_options(
		ClientOptions::parse_with_resolver_config(
			&env::var("MONGODB_URI")
				.expect("You must set the MONGODB_URI environment var!"),
			ResolverConfig::cloudflare(),
		)
		.await
		.expect("Failed to parse MONGODB_URI"),
	)
	.unwrap();

	let app_state = Arc::new(AtomicUsize::new(0));
	let snowflake_gen = Arc::new(Mutex::new(SnowflakeIdGenerator::with_epoch(
		1,
		1,
		UNIX_EPOCH + Duration::from_millis(1672531200),
	)));
	let server = ShikiServer::new(db.clone(), app_state.clone()).start();
	let listen_socket = "0.0.0.0:8081".parse::<SocketAddr>().unwrap();
	let webrtc_server = Server::new(listen_socket, listen_socket).await?;
	let session_endpoint =
		web::Data::new(Mutex::new(webrtc_server.session_endpoint()));

	log::info!("starting HTTP server at http://localhost:8080");

	let http_fut = HttpServer::new(move || {
		let cors = Cors::default()
			.allowed_origin(&env::var("CLIENT_URL").unwrap())
			.allowed_methods(vec!["GET", "POST"])
			.allowed_headers(vec![
				http::header::AUTHORIZATION,
				http::header::ACCEPT,
				http::header::CONTENT_TYPE,
			])
			.allowed_header(http::header::CONTENT_TYPE)
			.max_age(3600);

		App::new()
			.app_data(web::Data::from(app_state.clone()))
			.app_data(web::Data::new(server.clone()))
			.app_data(session_endpoint.clone())
			.app_data(web::Data::new(db.clone()))
			.app_data(web::Data::from(snowflake_gen.clone()))
			.app_data(web::JsonConfig::default().error_handler(|err, _req| {
				error::InternalError::from_response(
					"",
					HttpResponse::BadRequest()
						.content_type("application/json")
						.body(format!(r#"{{"error":"{}"}}"#, err)),
				)
				.into()
			}))
			.service(Files::new("/static", "./static"))
			.wrap(Logger::default())
			.wrap(cors)
			.configure(|cfg| {
				routes::routes(&db, cfg);
			})
	})
	.workers(2)
	.bind(("127.0.0.1", 8080))?
	.run();

	let webrtc_fut = recv_spin(webrtc_server);

	future::try_join(http_fut, webrtc_fut).await?;
	Ok(())
}

async fn recv_spin(mut webrtc_server: Server) -> std::io::Result<()> {
	let mut decoder =
		opus::Decoder::new(48000, opus::Channels::Stereo).unwrap();

	let mut message_buf: Vec<u8> = Vec::new();

	loop {
		let received = match webrtc_server.recv().await {
			Ok(received) => {
				message_buf.clear();
				message_buf.extend(received.message.as_ref());
				// message_buf.append("cool".to_string().into_bytes().as_mut());
				Some((received.message_type, received.remote_addr))
			}
			Err(err) => {
				log::error!("Could not receive RTC message: {}", err);
				None
			}
		};

		if let Some((message_type, remote_addr)) = received {
			// TODO: Keep all addrs together. Send their data to everyone but the sender.
			let result = process_packet(&mut decoder, &message_buf).await;

			if let Err(e) = result {
				log::error!("Could not decode message: {}", e);
				// continue;
			}

			if let Err(e) = webrtc_server
				.send(&message_buf, message_type, &remote_addr)
				.await
			{
				log::error!("Could not send message to {}: {}", remote_addr, e);
			}
		}
	}
}

async fn process_packet(
	_decoder: &mut opus::Decoder, packet: &[u8],
) -> std::io::Result<()> {
	log::debug!("Received {} bytes", packet.len());

	Ok(())
}
