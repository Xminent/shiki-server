use crate::{
	auth,
	models::{User, UserInsert, UserLogin, UserResponse},
	routes::{DB_NAME, USER_COLL_NAME},
};
use actix_web::{post, web, HttpResponse};
use mongodb::{
	bson::doc,
	error::{ErrorKind, WriteFailure},
	Client,
};
use snowflake::SnowflakeIdGenerator;
use std::sync::Mutex;
use validator::Validate;

// NOTE: One time setup for unique registration
// let email_index_model = IndexModel::builder()
//     .keys(doc! {"email": 1})
//     .options(IndexOptions::builder().unique(true).build())
//     .build();

// collection.create_index(email_index_model, None).await;

#[post("/register")]
async fn register(
	client: web::Data<Client>, data: web::Json<UserInsert>,
	snowflake_gen: web::Data<Mutex<SnowflakeIdGenerator>>,
) -> HttpResponse {
	if let Err(err) = data.validate() {
		return HttpResponse::BadRequest().json(err);
	}

	let id = snowflake_gen.lock().unwrap().real_time_generate();

	let user = User::new(
		id,
		&data.email,
		&data.username,
		&auth::hash(data.password.as_bytes()).await,
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

#[post("/login")]
async fn login(
	client: web::Data<Client>, data: web::Json<UserLogin>,
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
			match auth::verify_password(
				&user.password,
				data.password.as_bytes(),
			)
			.await
			{
				Ok(_) => HttpResponse::Ok().json(UserResponse::from(user)),
				Err(_) => HttpResponse::BadRequest().body("Invalid password"),
			}
		}

		_ => HttpResponse::InternalServerError().body("Something went wrong"),
	}
}

pub fn routes(cfg: &mut web::ServiceConfig) {
	cfg.service(web::scope("/auth").service(register).service(login));
}
