mod api;
mod auth;
mod gateway;
mod middleware;

use actix_web::web;
use mongodb::Client;

pub const DB_NAME: &str = "shiki";
pub const USER_COLL_NAME: &str = "users";

pub fn routes(client: &Client, cfg: &mut web::ServiceConfig) {
	cfg.configure(|cfg| {
		api::routes(client, cfg);
	});

	cfg.configure(auth::routes).configure(gateway::routes);
}
