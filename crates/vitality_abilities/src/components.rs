use crate::ability_state::AbilityMachine;
use crate::definition::AbilityDef;
use crate::lifecycle::AbilityEnding;
use crate::registry::{AbilityRegistry, AbilityToken};
use crate::tasks::Tasks;
use bevy_asset::Handle;
use bevy_ecs::prelude::*;
use bevy_ecs::template::TemplateContext;
use bevy_math::Vec3;
use bevy_reflect::prelude::Reflect;
use hfsm_bevy::MachineInstance;
use std::error::Error;
use std::fmt::Formatter;
use std::marker::PhantomPinned;

#[derive(Component, Clone, Reflect, Debug)]
#[relationship(relationship_target = GrantedAbilities)]
pub struct AbilityOf(pub Entity);

impl Default for AbilityOf {
    fn default() -> Self {
        Self(Entity::PLACEHOLDER)
    }
}

#[derive(Component, Reflect, Debug, Default)]
#[relationship_target(relationship = AbilityOf, linked_spawn)]
pub struct GrantedAbilities(Vec<Entity>);

impl GrantedAbilities {
    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.0.iter()
    }
}

#[derive(Component, Clone, Reflect)]
#[reflect(Component)]
#[require(Tasks, AbilityEnding, MachineInstance<AbilityMachine>)]
pub struct Ability {
    pub handle: Handle<AbilityDef>,
}

impl FromTemplate for Ability {
    type Template = AbilityAssetTemplate;
}

pub struct AbilityAssetTemplate {
    pub handle: AbilityToken,
    _pin: PhantomPinned,
}

impl Template for AbilityAssetTemplate {
    type Output = Ability;
    fn build_template(&self, context: &mut TemplateContext) -> Result<Self::Output> {
        let ability_registry = context
            .entity
            .world()
            .get_resource::<AbilityRegistry>()
            .expect("The AbilityRegistry must exist.");

        let handle = ability_registry.get(&self.handle);
        Ok(Ability {
            handle: handle.clone(),
        })
    }

    fn clone_template(&self) -> Self {
        AbilityAssetTemplate {
            handle: self.handle.clone_template(),
            _pin: Default::default(),
        }
    }
}

impl Default for AbilityAssetTemplate {
    fn default() -> Self {
        Self {
            handle: AbilityToken::new_static(""),
            _pin: PhantomPinned,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum TargetData {
    SelfCast,
    Target(Entity),
    Location(Vec3),
}

#[derive(EntityEvent)]
pub struct BeginAbility {
    pub source: Entity,
    #[event_target]
    pub ability: Entity,
}

#[derive(EntityEvent)]
pub struct ExecuteAbility {
    #[event_target]
    pub ability: Entity,
    pub target: TargetData,
    pub source: Entity,
}

#[derive(EntityEvent)]
pub struct EndAbility {
    #[event_target]
    pub ability: Entity,
    pub source: Entity,
}

#[derive(Clone, Debug)]
pub enum AbilityError {
    GrantingAbilityToNonActor(Entity),
    AbilityDoesNotExist(Entity),
}

impl std::fmt::Display for AbilityError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AbilityError::GrantingAbilityToNonActor(entity) => {
                write!(
                    f,
                    "{}: Cannot grant ability to an entity that is not an actor; grant the ability to an entity built by ActorBuilder.",
                    entity
                )
            }
            AbilityError::AbilityDoesNotExist(entity) => {
                write!(
                    f,
                    "{}: The entity is not an ability (e.g. No Ability component).",
                    entity
                )
            }
        }
    }
}

impl Error for AbilityError {}
