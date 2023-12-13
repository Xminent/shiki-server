use crate::{
	models::User,
	routes::{DB_NAME, USER_COLL_NAME},
	utils,
};
use actix_session::Session;
use actix_web::{get, post, web, HttpResponse};
use futures_util::lock::Mutex;
use mongodb::{
	bson::doc,
	error::{ErrorKind, WriteFailure},
	options::IndexOptions,
	Client, IndexModel,
};
use serde::{Deserialize, Serialize};
use snowflake::SnowflakeIdGenerator;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct UserInsert {
	#[validate(email)]
	pub email: String,
	#[validate(length(min = 3))]
	pub username: String,
	#[validate(length(min = 8))]
	pub password: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UserResponse {
	pub user_id: i64,
	pub token: String,
}

impl From<User> for UserResponse {
	fn from(user: User) -> Self {
		UserResponse { user_id: user.id, token: user.token }
	}
}

#[post("/register")]
async fn register(
	client: web::Data<Client>, data: web::Json<UserInsert>,
	snowflake_gen: web::Data<Mutex<SnowflakeIdGenerator>>,
) -> HttpResponse {
	if let Err(err) = data.validate() {
		return HttpResponse::BadRequest().json(err);
	}

	let id = snowflake_gen.lock().await.real_time_generate();

	let user = User::new(
		id,
		&data.email,
		&data.username,
		&utils::hash(data.password.as_bytes()).await,
	);

	let res = client
		.database(DB_NAME)
		.collection(USER_COLL_NAME)
		.insert_one(user.clone(), None)
		.await;

	match res {
		Ok(_) => HttpResponse::Ok().json(UserResponse::from(user)),
		Err(err) => {
			log::error!("add_user: {}", err);

			if let ErrorKind::Write(WriteFailure::WriteError(write_err)) =
				*err.kind
			{
				if write_err.code == 11000 {
					return HttpResponse::BadRequest()
						.body("User already exists");
				}
			}

			HttpResponse::InternalServerError().body("Something went wrong")
		}
	}
}

#[derive(Debug, Deserialize, Validate)]
struct UserLogin {
	#[validate(email)]
	pub email: String,
	#[validate(length(min = 8))]
	pub password: String,
}

#[post("/login")]
async fn login(
	client: web::Data<Client>, data: web::Json<UserLogin>, session: Session,
) -> HttpResponse {
	if let Err(err) = data.validate() {
		return HttpResponse::BadRequest().json(err);
	}

	// Verify if the user exists.
	let res = client
		.database(DB_NAME)
		.collection::<User>(USER_COLL_NAME)
		.find_one(doc! {"email": &data.email}, None)
		.await;

	match res {
		Ok(Some(user)) => {
			match utils::verify_password(
				&user.password,
				data.password.as_bytes(),
			)
			.await
			{
				Ok(_) => {
					session.insert("user", user.clone()).unwrap();
					session.renew();

					HttpResponse::Ok().json(UserResponse::from(user))
				}
				Err(_) => HttpResponse::BadRequest().body("Invalid password"),
			}
		}
		Ok(None) => HttpResponse::BadRequest().body("User not found"),
		Err(err) => {
			log::error!("login: {}", err);
			HttpResponse::InternalServerError().body("Something went wrong")
		}
	}
}

#[get("/user")]
async fn get_user(session: Session) -> HttpResponse {
	let user = session.get::<User>("user");

	match user {
		Ok(Some(user)) => HttpResponse::Ok().json(user),
		_ => HttpResponse::Unauthorized().body("Unauthorized"),
	}
}

pub async fn setup_indexes(client: &Client) -> anyhow::Result<()> {
	let email_index_model = IndexModel::builder()
		.keys(doc! {"email": 1})
		.options(IndexOptions::builder().unique(true).build())
		.build();

	match client
		.database(DB_NAME)
		.collection::<User>(USER_COLL_NAME)
		.create_index(email_index_model, None)
		.await
	{
		Ok(_) => (),
		Err(err) => match *err.kind {
			ErrorKind::ServerSelection { .. } => {
				return Err(anyhow::anyhow!("Not connected"));
			}
			_ => (),
		},
	}

	Ok(())
}

pub fn routes(cfg: &mut web::ServiceConfig) {
	cfg.service(web::scope("/auth").service(register).service(login));
}
