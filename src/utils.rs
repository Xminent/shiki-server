use crate::{
	models::User,
	redis::{FetchUserId, RedisFetcher},
};
use argon2::{
	password_hash::{
		rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier,
		SaltString,
	},
	Argon2,
};

pub async fn validate_token(
	fetcher: RedisFetcher, token: String,
) -> anyhow::Result<Option<User>> {
	uuid::Uuid::parse_str(&token)?;

	fetcher.fetch_user(FetchUserId::Token(token)).await.map_err(|e| {
		log::error!("Failed to validate token: {}", e);
		e
	})
}

pub async fn hash(password: &[u8]) -> String {
	let salt = SaltString::generate(&mut OsRng);

	Argon2::default()
		.hash_password(password, &salt)
		.expect("Unable to hash password.")
		.to_string()
}

pub async fn verify_password(
	hash: &str, password: &[u8],
) -> Result<(), argon2::password_hash::Error> {
	let parsed_hash = PasswordHash::new(hash)?;

	Argon2::default().verify_password(password, &parsed_hash)
}
