pub mod ability_state;
mod builder;
mod command;
mod system_param;
mod systems;
pub mod task_states;
pub mod tasks;

use crate::ability::ability_state::{setup_ability_machine_definition, AbilityMachine};
use crate::ability::systems::{activate_ability, tick_ability_cooldown};
use crate::ability::task_states::{setup_task_machine_definition, TaskMachine};
use crate::ability::tasks::{handles_wait_task_timers, on_task_completion_notification, Tasks};
use crate::assets::AbilityDef;
use crate::prelude::Attribute;
use crate::schedule::EffectsSet;
use crate::AttributeCalculatorCached;
use crate::ReflectAccessAttribute;
use bevy::prelude::*;
pub use builder::AbilityBuilder;
pub use command::GrantAbilityCommand;
use hfsm_bevy::MachineInstance;
use hfsm_bevy::StateMachinePlugin;
use num_traits::{AsPrimitive, Num};
use std::error::Error;
use std::fmt::Formatter;
use std::time::Duration;
pub use system_param::Abilities;

pub struct AbilityPlugin;

impl Plugin for AbilityPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(StateMachinePlugin::<AbilityMachine>::default())
            .add_plugins(StateMachinePlugin::<TaskMachine>::default())
            .add_systems(PreStartup, setup_ability_machine_definition)
            .add_systems(PreStartup, setup_task_machine_definition)
            .add_systems(Update, tick_ability_cooldown.in_set(EffectsSet::Prepare))
            .add_systems(PreUpdate, handles_wait_task_timers)
            .add_observer(activate_ability)
            .add_observer(on_task_completion_notification)
            .register_type::<AbilityOf>()
            .register_type::<GrantedAbilities>();
    }
}

/// The entity that this effect is targeting.
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = GrantedAbilities)]
pub struct AbilityOf(pub Entity);

/// All abilities granted to this entity.
#[derive(Component, Reflect, Debug, Default)]
#[relationship_target(relationship = AbilityOf, linked_spawn)]
pub struct GrantedAbilities(Vec<Entity>);

impl GrantedAbilities {
    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.0.iter()
    }
}

#[derive(Component, Reflect)]
#[reflect(Component)]
#[require(Tasks, AbilityRecovery, MachineInstance<AbilityMachine>)]
pub struct Ability(pub(crate) Handle<AbilityDef>);

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

/*attribute!(AbilityCooldown, f32);

impl Default for AbilityCooldown {
    fn default() -> Self {
        Self::new(1.0)
    }
}*/

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
