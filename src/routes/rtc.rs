use actix_web::{
	post,
	web::{self},
	HttpResponse,
};
use futures_util::lock::Mutex;
use webrtc_unreliable::SessionEndpoint;

#[post("/connect")]
async fn connect(
	se_mutex: web::Data<Mutex<SessionEndpoint>>, sdp: web::Payload,
) -> HttpResponse {
	let mut se = se_mutex.lock().await;

	match se.session_request(sdp).await {
		Ok(res) => HttpResponse::Ok()
			.append_header(("Content-Type", "application/json"))
			.body(res),
		Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
	}
}

pub fn routes(cfg: &mut web::ServiceConfig) {
	cfg.service(connect);
}
