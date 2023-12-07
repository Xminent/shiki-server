use crate::ws::server::ShikiServer;
use actix::*;
use actix_cors::Cors;
use actix_files::Files;
use actix_session::{
	storage::{RedisActorSessionStore, SessionStore},
	SessionMiddleware,
};
use actix_web::{
	cookie::Key,
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
use rtc::handler::Handlerr;
use snowflake::SnowflakeIdGenerator;
use std::{
	env,
	net::SocketAddr,
	sync::{atomic::AtomicUsize, Arc},
	time::{Duration, UNIX_EPOCH},
};
use webrtc_unreliable::Server;

mod errors;
mod models;
mod opus;
mod opusfile;
mod routes;
mod rtc;
mod speexdsp;
mod utils;
mod validator;
mod ws;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
	dotenv().expect("Failed to read .env file");

	env_logger::init_from_env(
		env_logger::Env::new().default_filter_or("debug"),
	);

	let session_key = env::var("SESSION_KEY")
		.expect("You must set the SESSION_KEY environment var!");

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

	let store = RedisActorSessionStore::new(
		&env::var("REDIS_URL").expect("REDIS_URL must be set"),
	);

	store
		.load(&session_key.clone().try_into().unwrap())
		.await
		.expect("Failed to connect to Redis");

	routes::setup_indexes(&db).await;

	let app_state = Arc::new(AtomicUsize::new(0));
	let snowflake_gen = Arc::new(Mutex::new(SnowflakeIdGenerator::with_epoch(
		1,
		1,
		UNIX_EPOCH + Duration::from_millis(1672531200),
	)));
	let server = ShikiServer::new(db.clone(), app_state.clone()).start();
	let listen_socket = "0.0.0.0:8081".parse::<SocketAddr>().unwrap();
	let public_addr = env::var("RTC_PUBLIC_ADDR")
		.expect("RTC_PUBLIC_ADDR must be set")
		.parse::<SocketAddr>()
		.expect("RTC_PUBLIC_ADDR must be a valid socket address");
	let webrtc_server = Server::new(listen_socket, public_addr).await?;
	let session_endpoint =
		web::Data::new(Mutex::new(webrtc_server.session_endpoint()));

	log::info!("starting HTTP server at http://localhost:8080");

	let http_fut = HttpServer::new(move || {
		let cors = Cors::default()
			.allowed_origin(
				&env::var("CLIENT_URL").expect("CLIENT_URL must be set"),
			)
			.allowed_methods(vec!["GET", "POST", "PATCH", "DELETE"])
			.allowed_headers(vec![
				http::header::AUTHORIZATION,
				http::header::ACCEPT,
			])
			.allowed_header(http::header::CONTENT_TYPE)
			.expose_headers(&[actix_web::http::header::CONTENT_DISPOSITION])
			.supports_credentials()
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
			.wrap(
				SessionMiddleware::builder(
					RedisActorSessionStore::new(
						&env::var("REDIS_URL").expect("REDIS_URL must be set"),
					),
					Key::from(session_key.as_bytes()),
				)
				.build(),
			)
			.wrap(Logger::default())
			.wrap(cors)
			.configure(|cfg| {
				routes::routes(&db, cfg);
			})
	})
	.workers(2)
	.bind(("127.0.0.1", 8080))?
	.run();

	let webrtc_server = Arc::new(Mutex::new(webrtc_server));
	let webrtc_fut = recv_spin(webrtc_server);

	future::try_join(http_fut, webrtc_fut).await?;
	Ok(())
}

async fn recv_spin(webrtc_server: Arc<Mutex<Server>>) -> std::io::Result<()> {
	let mut message_buf: Vec<u8> = Vec::new();
	let mut handler = Handlerr::new().map_err(|e| {
		log::error!("Could not create handler: {}", e);
		std::io::Error::new(
			std::io::ErrorKind::Other,
			"Could not create handler",
		)
	})?;

	loop {
		let received = match webrtc_server.lock().await.recv().await {
			Ok(received) => {
				message_buf.clear();
				message_buf.extend(received.message.as_ref());
				Some((received.message_type, received.remote_addr))
			}
			Err(err) => {
				log::error!("Could not receive RTC message: {}", err);
				None
			}
		};

		if let Some((message_type, remote_addr)) = received {
			// TODO: Keep all addrs together. Send their data to everyone but the sender.

			if let Err(e) = process_packet(
				&mut handler,
				&message_buf,
				message_type,
				webrtc_server.clone(),
				remote_addr.clone(),
			)
			.await
			{
				log::error!("Could not decode message: {}", e);
			}
		}
	}
}

async fn process_packet(
	handler: &mut Handlerr, packet: &[u8],
	_message_type: webrtc_unreliable::MessageType,
	_webrtc_server: Arc<Mutex<Server>>, _remote_addr: SocketAddr,
) -> anyhow::Result<()> {
	handler
		.process_packet(packet, |_packets| {
			// TODO: Do something with the audio packets if we wanted to. They are decoded and resampled here.
			// let server = webrtc_server.clone();

			// Box::pin(async move {
			// 	let u8_slice = unsafe {
			// 		std::mem::transmute::<_, &[u8]>(packets[0].as_slice())
			// 	};

			// 	match server
			// 		.lock()
			// 		.await
			// 		.send(u8_slice, message_type, &remote_addr)
			// 		.await
			// 	{
			// 		Ok(_) => {
			// 			log::debug!(
			// 				"Sent {} bytes to {}",
			// 				u8_slice.len(),
			// 				remote_addr
			// 			);
			// 		}
			// 		Err(e) => {
			// 			log::error!(
			// 				"Could not send packet to {}: {}",
			// 				remote_addr,
			// 				e
			// 			);
			// 		}
			// 	}
			// })

			Box::pin(async move {})
		})
		.await
}
