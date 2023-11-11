use actix_web::{FromRequest, HttpMessage};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::future::ready;
use validator::Validate;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct User {
	pub id: i64,
	pub email: String,
	pub username: String,
	pub password: String,
	/// The user's authentication token.
	pub token: String,
	/// Unix timestamp for when user was created.
	pub created_at: usize,
}

impl User {
	pub fn new(id: i64, email: &str, username: &str, password: &str) -> Self {
		User {
			id,
			email: email.to_string(),
			username: username.to_string(),
			password: password.to_string(),
			token: uuid::Uuid::new_v4().to_string(),
			created_at: Utc::now().timestamp() as usize,
		}
	}
}

impl FromRequest for User {
	type Error = actix_web::Error;
	type Future = std::future::Ready<Result<Self, Self::Error>>;

	fn from_request(
		req: &actix_web::HttpRequest, _: &mut actix_web::dev::Payload,
	) -> Self::Future {
		let extensions = req.extensions();
		let user = extensions.get::<User>();

		if let Some(user) = user {
			ready(Ok(user.clone()))
		} else {
			ready(Err(actix_web::error::ErrorUnauthorized("Unauthorized")))
		}
	}
}

#[derive(Debug, Deserialize, Validate)]
pub struct UserInsert {
	#[validate(email)]
	pub email: String,
	pub username: String,
	#[validate(length(min = 8))]
	pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UserLogin {
	#[validate(email)]
	pub email: String,
	#[validate(length(min = 8))]
	pub password: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UserResponse {
	pub email: String,
	pub username: String,
}

impl From<User> for UserResponse {
	fn from(user_db: User) -> Self {
		UserResponse { email: user_db.email, username: user_db.username }
	}
}
