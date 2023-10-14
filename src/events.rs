use actix::Message;
use serde::{Deserialize, Serialize};

// Opcode enum used for sending and receiving many of the Gateway events similar to the Discord Gateway.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum Opcode {
    Identify,
}

impl Opcode {
    pub fn from_json(value: Option<&serde_json::Value>) -> Option<Opcode> {
        value
            .and_then(|opcode| opcode.as_u64())
            .and_then(|opcode| match opcode {
                0 => Some(Opcode::Identify),
                _ => todo!(),
            })
    }
}

/// Identify Payload which is sent from the server to all clients notifying them someone is online.
#[derive(Message, Serialize, Deserialize, Debug)]
#[rtype(result = "()")]
pub struct Ready {
    pub id: i64,
    pub name: String,
}
