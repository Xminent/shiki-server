use crate::{server, session};
use actix::Addr;
use actix_web::{get, web, Error, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use std::time::Instant;

#[get("/gateway")]
async fn gateway(
    req: HttpRequest,
    stream: web::Payload,
    srv: web::Data<Addr<server::ShikiServer>>,
) -> Result<HttpResponse, Error> {
    ws::start(
        session::GatewaySession {
            id: 0,
            hb: Instant::now(),
            room: "main".to_owned(),
            name: None,
            addr: srv.get_ref().clone(),
            token: None,
        },
        &req,
        stream,
    )
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(gateway);
}
