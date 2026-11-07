use crate::ability_state::AbilityMachine;
use crate::definition::AbilityDef;
use crate::registry::{AbilityRegistry, AbilityToken};
use crate::tasks::Tasks;
use bevy::ecs::template::TemplateContext;
use bevy::prelude::*;
use hfsm_bevy::MachineInstance;
use num_traits::{AsPrimitive, Num};
use std::error::Error;
use std::fmt::Formatter;
use std::marker::PhantomPinned;
use std::time::Duration;
use vitality_core::attributes::ReflectAccessAttribute;
use vitality_core::modifier::AttributeCalculatorCached;
use vitality_effects::prelude::Attribute;

/// The entity that this effect is targeting.
#[derive(Component, Clone, Reflect, Debug)]
#[relationship(relationship_target = GrantedAbilities)]
pub struct AbilityOf(pub Entity);

impl Default for AbilityOf {
    fn default() -> Self {
        Self(Entity::PLACEHOLDER)
    }
}

/// All abilities granted to this entity.
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
#[require(Tasks, AbilityRecovery, MachineInstance<AbilityMachine>)]
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

#[derive(EntityEvent)]
pub struct AbilityCancel {
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
                    "{}: Cannot grant ability to entities that are not actors. with TypeId  not present on entity.",
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

#[derive(Component, Debug, Copy, Clone, Reflect, Default)]
#[require(AttributeCalculatorCached<AbilityRecovery>)]
#[reflect(Component, AccessAttribute)]
pub struct AbilityRecovery {
    duration: Duration,
}

impl AbilityRecovery {
    pub fn tick(&mut self, delta: Duration) {
        self.duration += delta;
    }
}

impl Attribute for AbilityRecovery {
    type Property = f32;

    fn new<T>(value: T) -> Self
    where
        T: Num + AsPrimitive<Self::Property> + Copy,
    {
        Self {
            duration: Duration::from_secs_f32(value.as_()),
        }
    }
    fn base_value(&self) -> f32 {
        self.duration.as_secs_f32()
    }
    fn base(&self) -> f32 {
        self.duration.as_secs_f32()
    }
    fn set_base_value(&mut self, value: f32) {
        self.duration = Duration::from_secs_f32(value);
    }
    fn current_value(&self) -> f32 {
        self.duration.as_secs_f32()
    }
    fn val(&self) -> f32 {
        self.duration.as_secs_f32()
    }
    fn set_current_value(&mut self, value: f32) {
        self.duration = Duration::from_secs_f32(value);
    }
}

impl std::fmt::Display for AbilityRecovery {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {:.1}",
            stringify!($StructName),
            self.duration.as_secs_f32()
        )
    }
}
