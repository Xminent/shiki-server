use mongodb::{bson::doc, Client};

use crate::{
	models::User,
	routes::{DB_NAME, USER_COLL_NAME},
};

pub async fn validate_token(client: Client, token: String) -> Option<User> {
	if uuid::Uuid::parse_str(&token).is_err() {
		return None;
	}

	match client
		.database(DB_NAME)
		.collection::<User>(USER_COLL_NAME)
		.find_one(doc! {"token": &token}, None)
		.await
	{
		Ok(Some(user)) => Some(user),
		_ => None,
	}
}
