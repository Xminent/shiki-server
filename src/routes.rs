use std::time::Instant;

use crate::{models::User, server, session};
use actix::Addr;
use actix_web::{get, post, web, Error, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use mongodb::{
    error::{ErrorKind, WriteFailure},
    Client,
};
use validator::Validate;

const DB_NAME: &str = "shiki";
const COLL_NAME: &str = "users";

// NOTE: One time setup for unique registration
// let email_index_model = IndexModel::builder()
//     .keys(doc! {"email": 1})
//     .options(IndexOptions::builder().unique(true).build())
//     .build();

// collection.create_index(email_index_model, None).await;

#[post("/auth/register")]
async fn add_user(
    client: web::Data<Client>,
    data: web::Json<User>,
) -> HttpResponse {
    if let Err(err) = data.validate() {
        return HttpResponse::BadRequest().json(err);
    }

    let collection = client.database(DB_NAME).collection(COLL_NAME);
    let result = collection.insert_one(data.clone(), None).await;

    match result {
        Ok(_) => HttpResponse::Ok().json(data),
        Err(err) => {
            log::error!("add_user: {}", err);

            match *err.kind {
                ErrorKind::Write(WriteFailure::WriteError(write_error)) => {
                    if write_error.code == 11000 {
                        return HttpResponse::BadRequest()
                            .body("User already exists".to_string());
                    }
                }
                _ => {}
            }

            HttpResponse::InternalServerError()
                .body("Something went wrong".to_string())
        }
    }
}

// Websocket Gateway Route
/// Entry point for our websocket route
#[get("/gateway")]
async fn gateway(
    req: HttpRequest,
    stream: web::Payload,
    srv: web::Data<Addr<server::ChatServer>>,
) -> Result<HttpResponse, Error> {
    ws::start(
        session::GatewaySession {
            id: 0,
            hb: Instant::now(),
            room: "main".to_owned(),
            name: None,
            addr: srv.get_ref().clone(),
        },
        &req,
        stream,
    )
}
