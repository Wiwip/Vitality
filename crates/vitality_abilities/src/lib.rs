mod ability_state;
mod command;
mod system_param;
mod systems;
mod task_states;
pub mod tasks;

use crate::command::on_add_ability;
use crate::systems::{activate_ability, tick_ability_cooldown};
use crate::tasks::{handles_wait_task_timers, on_task_completion_notification};
use bevy::prelude::*;
use hfsm_bevy::StateMachineAppExt;
use vitality_core::ability::ability_state::AbilityMachine;
use vitality_core::ability::task_states::TaskMachine;
use vitality_core::schedule::EffectsSet;

pub use system_param::Abilities;
pub use systems::ActivateAbility;

pub struct AbilityPlugin;

impl Plugin for AbilityPlugin {
    fn build(&self, app: &mut App) {
        // add_state_machine inserts the definition and the plugin together, so the definition is
        // never missing when a MachineInstance is first inserted.
        app.add_state_machine::<AbilityMachine>(ability_state::build_machine())
            .add_state_machine::<TaskMachine>(task_states::build_machine());

        app.add_systems(Update, tick_ability_cooldown.in_set(EffectsSet::Prepare))
            .add_systems(PreUpdate, handles_wait_task_timers);

        app.add_observer(activate_ability)
            .add_observer(on_task_completion_notification)
            .add_observer(on_add_ability);
    }
}
