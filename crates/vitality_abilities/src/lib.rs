pub mod ability_state;
mod builder;
mod command;
mod components;
pub mod condition;
pub mod context;
pub mod definition;
pub mod registry;
pub mod subject;
mod system_param;
mod systems;
pub mod task_states;
pub mod tasks;

use crate::ability_state::AbilityMachine;
use crate::command::on_add_ability;
use crate::systems::{activate_ability, tick_ability_cooldown};
use crate::task_states::TaskMachine;
use crate::tasks::{Tasks, WaitTask};
use crate::tasks::{handles_wait_task_timers, on_task_completion_notification};
use bevy::ecs::world::{EntityMutExcept, EntityRefExcept};
use bevy::prelude::*;
use hfsm_bevy::StateMachineAppExt;
use hfsm_bevy::{MachineInstance, StateTimer};
use vitality_core::schedule::EffectsSet;

pub use builder::AbilityBuilder;
pub use components::*;
pub use definition::AbilityDef;
pub use system_param::Abilities;
pub use systems::ActivateAbility;

pub struct AbilityPlugin;

impl Plugin for AbilityPlugin {
    fn build(&self, app: &mut App) {
        // add_state_machine inserts the definition and the plugin together, so the definition is
        // never missing when a MachineInstance is first inserted.
        app.init_asset::<AbilityDef>()
            .insert_resource(registry::AbilityRegistry::default());

        app.add_state_machine::<AbilityMachine>(ability_state::build_machine())
            .add_state_machine::<TaskMachine>(task_states::build_machine());

        app.add_systems(Update, tick_ability_cooldown.in_set(EffectsSet::Prepare))
            .add_systems(PreUpdate, handles_wait_task_timers);

        app.add_observer(activate_ability)
            .add_observer(on_task_completion_notification)
            .add_observer(on_add_ability);
    }
}

pub type AbilityMut<'w, 's> = EntityMutExcept<
    'w,
    's,
    (
        ChildOf,
        GrantedAbilities,
        Tasks,
        AbilityOf,
        AbilityRecovery,
        (
            MachineInstance<AbilityMachine>,
            MachineInstance<TaskMachine>,
            StateTimer<AbilityMachine>,
            StateTimer<TaskMachine>,
            WaitTask,
        ),
    ),
>;

pub type AbilityRef<'w, 's> = EntityRefExcept<
    'w,
    's,
    (
        ChildOf,
        GrantedAbilities,
        Tasks,
        AbilityOf,
        AbilityRecovery,
        (
            MachineInstance<AbilityMachine>,
            MachineInstance<TaskMachine>,
            StateTimer<AbilityMachine>,
            StateTimer<TaskMachine>,
            WaitTask,
        ),
    ),
>;
