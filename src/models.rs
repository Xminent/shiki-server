use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, Validate)]
pub struct User {
    #[validate(email)]
    pub email: String,
    pub username: String,
}
