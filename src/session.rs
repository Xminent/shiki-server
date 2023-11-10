use crate::{
	events::{self, Opcode},
	server,
};
use actix::prelude::*;
use actix_web_actors::ws::{self, Message};
use anyhow::Result;
use std::time::{Duration, Instant};

/// How often heartbeat pings are sent
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// How long to wait before lack of client response causes a timeout
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);

/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct GatewaySession {
	/// unique session id
	pub session_id: usize,
	/// Client must send ping at least once per 10 seconds (CLIENT_TIMEOUT),
	/// otherwise we drop connection.
	pub hb: Instant,
	/// joined channel
	pub channel: i64,
	/// peer id
	pub id: i64,
	/// peer name
	pub name: Option<String>,
	/// Chat server
	pub addr: Addr<server::ShikiServer>,
	/// Auth token
	pub token: Option<String>,
}

impl GatewaySession {
	/// helper method that sends ping to client every 5 seconds (HEARTBEAT_INTERVAL).
	/// also this method checks heartbeats from client
	fn heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
		ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
			// check client heartbeats
			if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
				// heartbeat timed out
				log::debug!(
					"Websocket Client heartbeat failed, disconnecting!"
				);

				// notify chat server
				act.addr.do_send(server::Disconnect { id: act.session_id });
				// stop actor
				ctx.stop();

				// don't try to send a ping
				return;
			}

			ctx.ping(b"");
		});
	}

	fn check_authenticate(&self, ctx: &mut ws::WebsocketContext<Self>) {
		ctx.run_later(AUTH_TIMEOUT, |act, ctx| {
			if act.token.is_none() {
				log::debug!(
					"Websocket Client authentication failed, disconnecting!"
				);
				act.addr.do_send(server::Disconnect { id: act.session_id });
				ctx.stop();
			}
		});
	}

	fn handle_json(&self, text: &str) -> Result<()> {
		let json = serde_json::from_str::<serde_json::Value>(text)?;
		let opcode = Opcode::from_json(json.get("op"))
			.ok_or(anyhow::anyhow!("no opcode"))?;
		let data = json.get("d").ok_or(anyhow::anyhow!("no data"))?;

		log::debug!("Received opcode: {:?}", opcode);
		log::debug!("Received data: {:?}", data);

		match opcode {
			Opcode::Identify => {
				let token = data
					.get("token")
					.and_then(|t| t.as_str())
					.ok_or(anyhow::anyhow!("no token"))?;

				self.addr.do_send(server::Identify {
					id: self.session_id,
					token: token.to_string(),
				});
			}

			_ => (),
		}

		Ok(())
	}
}

impl Actor for GatewaySession {
	type Context = ws::WebsocketContext<Self>;

	/// Method is called on actor start.
	/// We register ws session with ChatServer
	fn started(&mut self, ctx: &mut Self::Context) {
		// start checking for authentication
		// self.check_authenticate(ctx);
		// we'll start heartbeat process on session start.
		self.heartbeat(ctx);

		// register self in chat server. `AsyncContext::wait` register
		// future within context, but context waits until this future resolves
		// before processing any other events.
		// HttpContext::state() is instance of WsChatSessionState, state is shared
		// across all routes within application
		let addr = ctx.address();

		self.addr
			.send(server::Connect { addr: addr.recipient() })
			.into_actor(self)
			.then(|res, act, ctx| {
				match res {
					Ok(res) => act.session_id = res,
					// something is wrong with chat server
					_ => ctx.stop(),
				}
				fut::ready(())
			})
			.wait(ctx);
	}

	fn stopping(&mut self, _: &mut Self::Context) -> Running {
		// notify chat server
		self.addr.do_send(server::Disconnect { id: self.session_id });
		Running::Stop
	}
}

/// Handle messages from chat server, we simply send it to peer websocket
impl Handler<events::Event> for GatewaySession {
	type Result = ();

	fn handle(&mut self, msg: events::Event, ctx: &mut Self::Context) {
		log::debug!("session {} received {:?}", self.session_id, msg);

		match msg {
			events::Event::Custom(msg) => ctx.text(msg),

			ref other => {
				let opcode = other.opcode();
				// Create a JSON object with an  op being opcode and a key "d" containing the payload.
				let json = serde_json::json!({
					"op": opcode,
					"d": serde_json::to_value(other).unwrap(),
				});

				ctx.text(serde_json::to_string(&json).unwrap());
			}
		}
	}
}

/// Actual websocket message handler
impl StreamHandler<Result<Message, ws::ProtocolError>> for GatewaySession {
	fn handle(
		&mut self, msg: Result<Message, ws::ProtocolError>,
		ctx: &mut Self::Context,
	) {
		let msg = match msg {
			Err(_) => {
				ctx.stop();
				return;
			}
			Ok(msg) => msg,
		};

		match msg {
			Message::Ping(msg) => {
				self.hb = Instant::now();
				ctx.pong(&msg);
			}
			Message::Pong(_) => {
				self.hb = Instant::now();
			}
			Message::Text(text) => match self.handle_json(&text) {
				Ok(_) => (),
				Err(e) => {
					ctx.text(e.to_string());
					ctx.stop();
				}
			},
			Message::Binary(b) => {
				log::debug!("Received len {} bytes", b.len());
				ctx.binary(b);
			}
			Message::Close(reason) => {
				ctx.close(reason);
				ctx.stop();
			}
			Message::Continuation(_) => {
				ctx.stop();
			}
			Message::Nop => (),
		}
	}
}
