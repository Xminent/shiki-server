use std::{
    env,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use actix::*;
use actix_cors::Cors;
use actix_files::Files;
use actix_web::{
    get, http, middleware::Logger, web, App, HttpServer, Responder,
};
use dotenv::dotenv;
use mongodb::{
    options::{ClientOptions, ResolverConfig},
    Client,
};

mod models;
mod routes;
mod server;
mod session;

/// Displays state
#[get("/count")]
async fn get_count(count: web::Data<AtomicUsize>) -> impl Responder {
    let current_count = count.load(Ordering::SeqCst);
    format!("Visitors: {current_count}")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let client_uri = env::var("MONGODB_URI")
        .expect("You must set the MONGODB_URI environment var!");

    let options = ClientOptions::parse_with_resolver_config(
        &client_uri,
        ResolverConfig::cloudflare(),
    )
    .await
    .expect("Failed to parse MONGODB_URI");

    let db = Client::with_options(options).unwrap();

    env_logger::init_from_env(
        env_logger::Env::new().default_filter_or("debug"),
    );

    // set up applications state
    // keep a count of the number of visitors
    let app_state = Arc::new(AtomicUsize::new(0));

    // start chat server actor
    let server = server::ChatServer::new(app_state.clone()).start();

    log::info!("starting HTTP server at http://localhost:8080");

    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin("http://localhost:3000")
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![
                http::header::AUTHORIZATION,
                http::header::ACCEPT,
            ])
            .allowed_header(http::header::CONTENT_TYPE)
            .max_age(3600);

        App::new()
            .app_data(web::Data::from(app_state.clone()))
            .app_data(web::Data::new(server.clone()))
            .app_data(web::Data::new(db.clone()))
            .service(routes::add_user)
            .service(get_count)
            .service(routes::gateway)
            .service(Files::new("/static", "./static"))
            .wrap(Logger::default())
            .wrap(cors)
    })
    .workers(2)
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
