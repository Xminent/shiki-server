use crate::{
	models::User,
	routes::{DB_NAME, USER_COLL_NAME},
};
use argon2::{
	password_hash::{
		rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier,
		SaltString,
	},
	Argon2,
};
use futures::TryStreamExt;
use mongodb::{bson::doc, Client};

pub async fn validate_token(
	client: Client, token: String,
) -> anyhow::Result<Option<User>> {
	uuid::Uuid::parse_str(&token)?;

	match client
		.database(DB_NAME)
		.collection::<User>(USER_COLL_NAME)
		.find_one(doc! {"token": &token}, None)
		.await
	{
		Ok(Some(user)) => Ok(Some(user)),
		Ok(None) => Err(anyhow::anyhow!("Invalid token")),
		Err(e) => {
			log::error!("Failed to validate token: {}", e);

			Err(e.into())
		}
	}
}

pub async fn get_all_users(client: Client) -> Vec<User> {
	client
		.database(DB_NAME)
		.collection::<User>(USER_COLL_NAME)
		.find(None, None)
		.await
		.unwrap()
		.try_collect()
		.await
		.unwrap()
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
