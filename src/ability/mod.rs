pub mod ability_state;
mod builder;
mod command;
mod system_param;
mod systems;
pub mod task_states;
pub mod tasks;

use crate::AttributeCalculatorCached;
use crate::ReflectAccessAttribute;
use crate::ability::ability_state::{AbilityMachine, setup_ability_machine_definition};
use crate::ability::command::on_add_ability;
use crate::ability::systems::{activate_ability, tick_ability_cooldown};
use crate::ability::task_states::{TaskMachine, setup_task_machine_definition};
use crate::ability::tasks::{Tasks, handles_wait_task_timers, on_task_completion_notification};
use crate::assets::AbilityDef;
use crate::prelude::Attribute;
use crate::registry::ability_registry::{AbilityRegistry, AbilityToken};
use crate::schedule::EffectsSet;
use bevy::ecs::template::TemplateContext;
use bevy::prelude::*;
pub use builder::AbilityBuilder;
use hfsm_bevy::MachineInstance;
use hfsm_bevy::StateMachinePlugin;
use num_traits::{AsPrimitive, Num};
use std::error::Error;
use std::fmt::Formatter;
use std::marker::PhantomPinned;
use std::time::Duration;
pub use system_param::Abilities;

pub struct AbilityPlugin;

impl Plugin for AbilityPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(StateMachinePlugin::<AbilityMachine>::default())
            .add_plugins(StateMachinePlugin::<TaskMachine>::default());

        app.add_systems(PreStartup, setup_ability_machine_definition)
            .add_systems(PreStartup, setup_task_machine_definition)
            .add_systems(Update, tick_ability_cooldown.in_set(EffectsSet::Prepare))
            .add_systems(PreUpdate, handles_wait_task_timers);

        app.add_observer(activate_ability)
            .add_observer(on_task_completion_notification)
            .add_observer(on_add_ability);
    }
}

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
