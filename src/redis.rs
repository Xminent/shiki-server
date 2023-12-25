// Methods which abstract the fetching of our data from redis, all data should be fetched first on redist, and fallback to the database.

use crate::{
	models,
	routes::{CHANNEL_COLL_NAME, DB_NAME, USER_COLL_NAME},
};
use anyhow::Result;
use deadpool_redis::{
	redis::{AsyncCommands, FromRedisValue, ToRedisArgs},
	Connection, Pool,
};
use futures_util::TryStreamExt;
use mongodb::{bson::doc, Client};
use serde::Deserialize;
use validator::Validate;

async fn get_value<T>(conn: &mut Connection, key: &str) -> Result<T>
where
	T: FromRedisValue,
{
	conn.hgetall(key).await.map_err(|e| anyhow::anyhow!(e))
}

async fn set_value<T>(conn: &mut Connection, key: &str, value: &T) -> Result<()>
where
	T: ToRedisArgs,
{
	deadpool_redis::redis::cmd("HSET")
		.arg(key)
		.arg(value)
		.query_async(conn)
		.await
		.map_err(|e| anyhow::anyhow!(e))
}

#[derive(Clone)]
pub struct RedisFetcher {
	client: Client,
	session: Pool,
}

impl std::fmt::Debug for RedisFetcher {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("RedisFetcher").field("client", &self.client).finish()
	}
}

#[derive(Clone)]
pub enum FetchUserId {
	Id(i64),
	Token(String),
}

impl RedisFetcher {
	pub fn new(client: Client, session: Pool) -> Self {
		Self { client, session }
	}

	async fn create_connection(&self) -> Result<Connection> {
		self.session.get().await.map_err(|e| anyhow::anyhow!(e))
	}

	pub async fn fetch_channels(
		&self, ids: Option<&[i64]>,
	) -> Result<Vec<models::Channel>> {
		let mut conn = self.create_connection().await?;
		let mut channels = Vec::new();
		let mut doc = None;

		if let Some(ids) = ids {
			let mut ids_to_fetch = Vec::new();

			for id in ids {
				if let Ok(channel) = get_value::<models::Channel>(
					&mut conn,
					&format!("channel_{id}"),
				)
				.await
				{
					channels.push(channel);
				} else {
					ids_to_fetch.push(id);
				}
			}

			if ids_to_fetch.is_empty() {
				return Ok(vec![]);
			}

			doc = Some(doc! {"id": {"$in": ids_to_fetch}});
		}

		let res = self
			.client
			.database(DB_NAME)
			.collection::<models::Channel>(CHANNEL_COLL_NAME)
			.find(doc, None)
			.await;

		match res {
			Ok(mut cursor) => {
				while let Some(channel) = cursor.try_next().await? {
					set_value(
						&mut conn,
						&format!("channel_{}", channel.id),
						&channel,
					)
					.await?;
					channels.push(channel);
				}

				Ok(channels)
			}
			Err(e) => Err(e.into()),
		}
	}

	pub async fn fetch_messages(
		&self, ids: &[i64],
	) -> Result<Vec<models::Message>> {
		let mut conn = self.create_connection().await?;
		let mut ids_to_fetch = Vec::new();
		let mut messages = Vec::new();

		for id in ids {
			if let Ok(message) = get_value::<models::Message>(
				&mut conn,
				&format!("message_{id}"),
			)
			.await
			{
				messages.push(message);
			} else {
				ids_to_fetch.push(id);
			}
		}

		if ids_to_fetch.is_empty() {
			return Ok(messages);
		}

		let res = self
			.client
			.database(DB_NAME)
			.collection::<models::Message>(USER_COLL_NAME)
			.find(doc! {"id": {"$in": ids_to_fetch}}, None)
			.await;

		match res {
			Ok(mut cursor) => {
				while let Some(message) = cursor.try_next().await? {
					set_value(
						&mut conn,
						&format!("message_{}", message.id),
						&message,
					)
					.await?;
					messages.push(message);
				}
				Ok(messages)
			}
			Err(e) => Err(e.into()),
		}
	}

	pub async fn fetch_user(
		&self, id: FetchUserId,
	) -> Result<Option<models::User>> {
		let mut conn = self.create_connection().await?;
		let doc = match id.clone() {
			FetchUserId::Id(id) => {
				let query = format!("user_{id}");

				if let Ok(user) =
					get_value::<models::User>(&mut conn, &query).await
				{
					return Ok(Some(user));
				}

				doc! {"id": id}
			}
			FetchUserId::Token(token) => {
				match conn.get::<_, i64>(&format!("user_token_{token}")).await {
					Ok(user_id) => {
						let query = format!("user_{user_id}");
						log::debug!("found user_token_{token} in cache");

						match get_value::<models::User>(&mut conn, &query).await
						{
							Ok(user) => {
								log::debug!(
									"fetched user {user_id} from cache, {:?}",
									user
								);
								return Ok(Some(user));
							}
							Err(e) => {
								log::debug!(
									"failed to get {query} from cache: {e}"
								);
								doc! {"id": user_id}
							}
						}
					}

					Err(e) => {
						log::debug!("error in redis fetch user: {e}");
						doc! {"token": token}
					}
				}
			}
		};

		let res = self
			.client
			.database(DB_NAME)
			.collection::<models::User>(USER_COLL_NAME)
			.find_one(doc, None)
			.await;

		match res {
			Ok(Some(user)) => {
				set_value(&mut conn, &format!("user_{}", user.id), &user)
					.await?;
				conn.set(&format!("user_token_{}", user.token), user.id)
					.await?;
				log::debug!(
					"cached both user_{} and user_token_{}",
					user.id,
					user.token
				);
				Ok(Some(user))
			}
			Ok(None) => Ok(None),
			Err(e) => Err(e.into()),
		}
	}

	pub async fn fetch_users(
		&self, ids: Option<&[i64]>,
	) -> Result<Vec<models::User>> {
		let mut conn = self.create_connection().await?;
		let mut doc = None;
		let mut users = Vec::new();

		if let Some(ids) = ids {
			let mut ids_to_fetch = Vec::new();

			for id in ids {
				if let Ok(user) =
					get_value::<models::User>(&mut conn, &format!("user_{id}"))
						.await
				{
					users.push(user);
				} else {
					ids_to_fetch.push(id);
				}
			}

			if ids_to_fetch.is_empty() {
				return Ok(users);
			}

			doc = Some(doc! {"id": {"$in": ids_to_fetch}});
		}

		let db_users = self
			.client
			.database(DB_NAME)
			.collection::<models::User>(USER_COLL_NAME)
			.find(doc, None)
			.await?
			.try_collect::<Vec<_>>()
			.await?;

		for user in db_users {
			set_value(&mut conn, &format!("user_{}", user.id), &user).await?;
			users.push(user);
		}

		Ok(users)
	}

	pub async fn modify_user(
		&self, user: &mut models::User, data: ModifyUser,
	) -> Result<()> {
		let id = user.id;
		let mut conn = self.create_connection().await?;

		if let Some(ref username) = data.username {
			user.username = username.clone();
		}

		if let Some(ref avatar) = data.avatar {
			user.avatar = Some(avatar.clone());
		}

		if get_value::<models::User>(&mut conn, &format!("user_{}", user.id))
			.await
			.is_ok()
		{
			let mut fields: Vec<(String, String)> = vec![];

			if let Some(ref username) = data.username {
				fields.push(("username".to_string(), username.clone()));
			}

			if let Some(ref avatar) = data.avatar {
				fields.push(("avatar".to_string(), avatar.clone()));
			}

			conn.hset_multiple::<_, _, _, ()>(
				format!("user_{id}"),
				fields.as_slice(),
			)
			.await
			.map(|_| {
				log::debug!("modified user {id} in cache");
			})
			.map_err(|e| anyhow::anyhow!(e))?;
		}

		let mut fields = doc! {};

		if let Some(username) = data.username {
			fields.insert("username", username);
		}

		if let Some(avatar) = data.avatar {
			fields.insert("avatar", avatar);
		}

		self.client
			.database(DB_NAME)
			.collection::<models::User>(USER_COLL_NAME)
			.update_one(doc! {"id": id}, doc! {"$set": fields}, None)
			.await
			.map(|_| {
				log::debug!("modified user {} in db", id);
			})
			.map_err(|e| anyhow::anyhow!(e))
	}
}

#[derive(Deserialize, Validate)]
pub struct ModifyUser {
	/// User's username
	#[validate(length(min = 3, max = 20))]
	pub username: Option<String>,
	/// User's avatar
	pub avatar: Option<String>,
}
