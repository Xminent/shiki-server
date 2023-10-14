// use crate::auth;
// use actix_web::{dev::ServiceRequest, Error};
// use actix_web_httpauth::extractors::bearer::{BearerAuth, Config};
// use actix_web_httpauth::extractors::AuthenticationError;

// pub async fn validator(
//     req: ServiceRequest,
//     credentials: BearerAuth,
// ) -> Result<ServiceRequest, Error> {
//     let config = req
//         .app_data::<Config>()
//         .map(|data| data.get_ref().clone())
//         .unwrap_or_else(Default::default);

//     match auth::validate_token(credentials.token()) {
//         Ok(res) => {
//             if res == true {
//                 return Ok(req);
//             }

//             ()
//         }

//         Err(_) => Err(AuthenticationError::from(config).into()),
//     }
// }
