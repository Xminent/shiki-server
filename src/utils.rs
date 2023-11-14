use futures::TryStreamExt;
use mongodb::{bson::doc, Client};

use crate::{
	models::User,
	routes::{DB_NAME, USER_COLL_NAME},
};

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
