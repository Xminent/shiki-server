mod api;
mod auth;
mod gateway;

use actix_web::web;

pub const DB_NAME: &str = "shiki";
pub const USER_COLL_NAME: &str = "users";

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.configure(api::routes)
        .configure(auth::routes)
        .configure(gateway::routes);
}
