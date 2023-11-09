use actix::*;
use actix_cors::Cors;
use actix_files::Files;
use actix_web::{
	error, http, middleware::Logger, web, App, HttpResponse, HttpServer,
};
use dotenv::dotenv;
use mongodb::{
	options::{ClientOptions, ResolverConfig},
	Client,
};
use snowflake::SnowflakeIdGenerator;
use std::{
	env,
	sync::{atomic::AtomicUsize, Arc, Mutex},
	time::{Duration, UNIX_EPOCH},
};

mod auth;
mod errors;
mod events;
mod models;
mod routes;
mod server;
mod session;
mod utils;
mod validator;

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
	let server = server::ShikiServer::new(
		db.clone(),
		snowflake_gen.clone(),
		app_state.clone(),
	)
	.start();

	log::info!("starting HTTP server at http://localhost:8080");

	HttpServer::new(move || {
		// let auth = HttpAuthentication::bearer(validator::validator);

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
            .service(Files::new("/assets", "./assets"))
            .wrap(Logger::default())
            // .wrap(auth)
            .wrap(cors)
            .configure(routes::routes)
	})
	.workers(2)
	.bind(("127.0.0.1", 8080))?
	.run()
	.await
}
