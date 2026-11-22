pub mod ability_state;
mod builder;
mod command;
mod components;
pub mod condition;
pub mod context;
pub mod def_file;
pub mod definition;
pub mod lifecycle;
pub mod registry;
mod system_param;
mod systems;
pub mod task_states;
pub mod tasks;

use crate::ability_state::AbilityMachine;
use crate::command::on_add_ability;
use crate::systems::activate_ability;
use crate::task_states::TaskMachine;
use crate::tasks::{Cast, Composite, OnFail, Progress, Tasks, WaitTask};
use crate::tasks::{handles_wait_task_timers, on_child_settled, on_start_composite};
use bevy_app::prelude::*;
use bevy_asset::AssetApp;
use bevy_ecs::prelude::*;
use bevy_ecs::world::{EntityMutExcept, EntityRefExcept};
use hfsm_bevy::MachineInstance;
use hfsm_bevy::StateMachineAppExt;

pub mod prelude {
    pub use crate::context::AbilityExprSchema;
}

pub use builder::AbilityBuilder;
pub use components::*;
pub use def_file::{DefFileError, DefNames, ability_def_from_file, ability_def_from_str};
pub use definition::AbilityDef;
// the cooldown is a StateTimer entry, so callers outside this crate need the name to read it
pub use hfsm_bevy::StateTimer;
pub use lifecycle::{
    AbilityActivated, AbilityActivationFailed, AbilityEnded, ActivationError, cooldown_remaining,
};
pub use system_param::Abilities;
pub use systems::ActivateAbility;

pub struct AbilityPlugin;

impl Plugin for AbilityPlugin {
    fn is_unique(&self) -> bool {
        false
    }

    fn build(&self, app: &mut App) {
        if !vitality_core::first_install::<Self>(app) {
            return;
        }
        // add_state_machine inserts the definition and the plugin together, so the definition is never missing when a MachineInstance is first inserted
        app.init_asset::<AbilityDef>()
            .insert_resource(registry::AbilityRegistry::default());

        app.add_state_machine::<AbilityMachine>(ability_state::build_machine())
            .add_state_machine::<TaskMachine>(task_states::build_machine());

        app.add_systems(PreUpdate, handles_wait_task_timers);

        app.add_observer(activate_ability)
            .add_observer(on_start_composite)
            .add_observer(on_child_settled)
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
        lifecycle::AbilityEnding,
        (
            MachineInstance<AbilityMachine>,
            MachineInstance<TaskMachine>,
            StateTimer<AbilityMachine>,
            StateTimer<TaskMachine>,
            WaitTask,
        ),
        (Composite, OnFail, Progress, Cast),
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
        lifecycle::AbilityEnding,
        (
            MachineInstance<AbilityMachine>,
            MachineInstance<TaskMachine>,
            StateTimer<AbilityMachine>,
            StateTimer<TaskMachine>,
            WaitTask,
        ),
        (Composite, OnFail, Progress, Cast),
    ),
>;
