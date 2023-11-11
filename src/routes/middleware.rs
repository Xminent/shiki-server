use crate::{models::User, utils::validate_token};
use actix_web::{
	dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
	HttpMessage,
};
use actix_web::{error::ErrorUnauthorized, Error};
use futures_util::future::LocalBoxFuture;
use mongodb::Client;
use std::{
	future::{ready, Ready},
	rc::Rc,
};

pub struct Auth {
	client: Client,
}

impl Auth {
	pub fn new(client: Client) -> Self {
		Auth { client }
	}
}

impl<S, B> Transform<S, ServiceRequest> for Auth
where
	S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>
		+ 'static,
	S::Future: 'static,
	B: 'static,
{
	type Response = ServiceResponse<B>;
	type Error = Error;
	type InitError = ();
	type Transform = AuthMiddleWare<S>;
	type Future = Ready<Result<Self::Transform, Self::InitError>>;

	fn new_transform(&self, service: S) -> Self::Future {
		ready(Ok(AuthMiddleWare {
			client: self.client.clone(),
			service: Rc::new(service),
		}))
	}
}

pub struct AuthMiddleWare<S> {
	client: Client,
	service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for AuthMiddleWare<S>
where
	S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>
		+ 'static,
	S::Future: 'static,
	B: 'static,
{
	type Response = ServiceResponse<B>;
	type Error = Error;
	type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

	forward_ready!(service);

	fn call(&self, req: ServiceRequest) -> Self::Future {
		let authorization = req.headers().get("Authorization");

		if authorization.is_none() {
			return Box::pin(async {
				Err(ErrorUnauthorized("Missing authorization header"))
			});
		}

		let token = authorization.and_then(|a| {
			let str = a.to_str();

			if str.is_err() {
				return None;
			}

			let str = str.unwrap();

			if !str.starts_with("Bearer ") {
				return None;
			}

			Some(str[7..].to_string())
		});

		if token.is_none() {
			return Box::pin(async {
				Err(ErrorUnauthorized(
					"Invalid authorization value. Must be 'Bearer <token>'",
				))
			});
		}

		let client_clone = self.client.clone();
		let token = token.unwrap();
		let service_clone = self.service.clone();

		Box::pin(async move {
			let user = validate_token(client_clone, token).await;

			if user.is_none() {
				return Err(ErrorUnauthorized("Invalid token"));
			}

			req.extensions_mut().insert::<User>(user.unwrap());

			let res = service_clone.call(req).await?;
			Ok(res)
		})
	}
}
