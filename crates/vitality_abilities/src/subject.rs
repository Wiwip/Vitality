use bevy::prelude::Reflect;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::fmt::{Display, Formatter};

#[derive(Copy, Clone, Debug, Reflect, Serialize, Deserialize)]
pub enum AbilitySubject {
    Caster,
    Ability,
    Target,
}

impl Display for AbilitySubject {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AbilitySubject::Caster => write!(f, "caster"),
            AbilitySubject::Ability => write!(f, "ability"),
            AbilitySubject::Target => write!(f, "target"),
        }
    }
}

impl From<AbilitySubject> for SmolStr {
    fn from(value: AbilitySubject) -> Self {
        SmolStr::from(value.to_string())
    }
}
