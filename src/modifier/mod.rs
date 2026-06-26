mod calculator;
mod events;
pub mod modifier;

use crate::inspector::pretty_type_name;
use crate::prelude::*;
use bevy::prelude::{Component, Entity, EntityCommands, Reflect, reflect_trait};
pub use calculator::{AttributeCalculator, AttributeCalculatorCached, ModOp};
pub use events::{ApplyAttributeModifierMessage, apply_modifier_events};
pub use modifier::AttributeModifier;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::fmt::{Debug, Display, Formatter};

pub type ModifierFn = dyn Fn(&mut EntityCommands, Entity) + Send + Sync;

#[derive(Component, Default, Copy, Clone, Debug, Reflect)]
pub struct ModifierMarker;

#[reflect_trait] // Generates a `ReflectMyTrait` type
pub trait AccessModifier {
    fn describe(&self) -> String;
    fn name(&self) -> String;
}

impl<T> AccessModifier for AttributeModifier<T>
where
    T: Attribute,
{
    fn describe(&self) -> String {
        format!("{}", self)
    }
    fn name(&self) -> String {
        pretty_type_name::<T>()
    }
}

#[derive(Copy, Clone, Debug, Reflect, Serialize, Deserialize)]
pub enum EffectSubject {
    Target,
    Source,
    Effect,
}

impl Display for EffectSubject {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            EffectSubject::Target => write!(f, "target"),
            EffectSubject::Source => write!(f, "source"),
            EffectSubject::Effect => write!(f, "effect"),
        }
    }
}

/*impl TryFrom<&Path> for EffectSubject {
    type Error = String;
    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        let root = path.0.split('.').next().unwrap_or("");
        match root.to_lowercase().as_str() {
            "target" => Ok(EffectSubject::Target),
            "src" | "source" => Ok(EffectSubject::Source),
            "effect" => Ok(EffectSubject::Effect),
            _ => Err(format!("'{}' is not a valid EffectSubject", root)),
        }
    }
}*/

impl From<EffectSubject> for SmolStr {
    fn from(value: EffectSubject) -> Self {
        SmolStr::from(value.to_string())
    }
}

#[derive(Copy, Clone, Debug, Reflect, Serialize, Deserialize)]
pub enum AbilitySubject {
    Caster,
    Ability,
    Target,
}

/*impl TryFrom<&Path> for AbilitySubject {
    type Error = String;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        let root = path
            .0
            .split('.')
            .next()
            .ok_or_else(|| format!("Invalid path format: {}", path.0))?;

        match root.to_lowercase().as_str() {
            "source" | "src" | "caster" => Ok(AbilitySubject::Caster),
            "ability" => Ok(AbilitySubject::Ability),
            "dst" | "target" => Ok(AbilitySubject::Target),
            _ => Err(format!("Unknown subject alias: {}", root)),
        }
    }
}*/

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

#[derive(Copy, Clone, Debug, Reflect, Serialize, Deserialize)]
pub enum ActorSubject {
    Actor,
}

/*impl TryFrom<&Path> for ActorSubject {
    type Error = String;
    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        let root = path.0.split('.').next().unwrap_or("");
        match root.to_lowercase().as_str() {
            "actor" => Ok(ActorSubject::Actor),
            _ => Err(format!("'{}' is not a valid ActorSubject", root)),
        }
    }
}*/

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

/// The target entity of this effect.
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = OwnedModifiers)]
pub struct ModifierOf(pub Entity);

/// All modifiers belonging to this effect.
#[derive(Component, Reflect, Debug)]
#[relationship_target(relationship = ModifierOf, linked_spawn)]
pub struct OwnedModifiers(Vec<Entity>);
