mod calculator;

use bevy::prelude::Reflect;
pub use calculator::{AttributeCalculator, AttributeCalculatorCached, ModOp};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::fmt::{Display, Formatter};

#[derive(Copy, Clone, Debug, Reflect, Serialize, Deserialize)]
pub enum ActorSubject {
    Actor,
}

impl Display for ActorSubject {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorSubject::Actor => write!(f, "actor"),
        }
    }
}

impl From<ActorSubject> for SmolStr {
    fn from(value: ActorSubject) -> Self {
        SmolStr::from(value.to_string())
    }
}
