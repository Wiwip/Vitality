use crate::AttributesRef;
use crate::ability::systems::{ActivateAbility, can_activate_ability};
use crate::ability::task_states::{TaskEvent, TaskMachine, TaskState};
use crate::ability::tasks::Tasks;
use crate::ability::{
    Ability, AbilityCooldown, AbilityRecovery, BeginAbility, ExecuteAbility, GrantedAbilities,
    TargetData,
};
use crate::actors::Actor;
use crate::assets::AbilityDef;
use crate::attributes::Attribute;
use crate::context::{AbilityExprContext, ActorExprContext};
use crate::registry::Registry;
use bevy::asset::Assets;
use bevy::ecs::query::QueryData;
use bevy::ecs::resource::IsResource;
use bevy::ecs::system::SystemParam;
use bevy::ecs::system::lifetimeless::{Read, Write};
use bevy::log::{error, warn};
use bevy::prelude::{Commands, Entity, Query, RelationshipTarget, Res, Without, debug};
use express_it::expr::BoolExpr;
use hfsm_bevy::{
    Access, EventResult, ExternalContext, LocalContext, Machine, MachineDefinition, MachineQuery,
    MachineState, StateId, StateTimer,
};

pub struct AbilityMachine;
impl Machine for AbilityMachine {
    type Local = AbilityContext;
    type External = AbilitySystemParam<'static, 'static>;
    type Event = AbilityEvent;
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AbilityState {
    _Root,
    Ready,
    Active,
    Recovery,
}
impl From<AbilityState> for StateId {
    fn from(value: AbilityState) -> Self {
        Self::try_from(value as u16).unwrap()
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
pub struct AbilityContext {
    ability_id: Entity,
    recovery_timer: Write<AbilityRecovery>,
    cooldown: Read<AbilityCooldown>,
    timers: Write<StateTimer<AbilityMachine>>,
}
impl LocalContext for AbilityContext {
    type Item<'w, 's> = <Self as QueryData>::Item<'w, 's>;
}

#[derive(SystemParam)]
pub struct AbilitySystemParam<'w, 's> {
    pub abilities:
        Query<'w, 's, (Read<Ability>, AttributesRef<'static, 'static>), Without<IsResource>>,
    pub actors: Query<
        'w,
        's,
        (
            Read<Actor>,
            AttributesRef<'static, 'static>,
            Read<GrantedAbilities>,
        ),
        Without<IsResource>,
    >,
    pub tasks: Query<'w, 's, Read<Tasks>>,
    pub task_machines: MachineQuery<'w, 's, TaskMachine>,
    pub registry: Registry<'w>,
    pub ability_assets: Res<'w, Assets<AbilityDef>>,
    pub commands: Commands<'w, 's>,
}
impl ExternalContext for AbilitySystemParam<'static, 'static> {
    type Item<'w, 's> = AbilitySystemParam<'w, 's>;
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum AbilityEvent {
    TryActivate { source: Entity, target: TargetData },
    Activate,
    CancelAbility,
    EndAbility,
    TimerExpired,
    Recovered,
}

fn build_machine() -> MachineDefinition<AbilityMachine> {
    MachineDefinition::<AbilityMachine>::builder(AbilityState::Ready, |root| {
        root.leaf(AbilityState::Ready, "Ready", ReadyState)
            .on(AbilityEvent::Activate, AbilityState::Active);

        root.leaf(AbilityState::Active, "Active", ActiveState)
            .on(AbilityEvent::EndAbility, AbilityState::Recovery)
            .on(AbilityEvent::CancelAbility, AbilityState::Recovery);

        root.leaf(AbilityState::Recovery, "Cooldown", RecoveryState)
            .on(AbilityEvent::Recovered, AbilityState::Ready);
    })
    .build()
    .expect("Failed to build HFSM")
    .into()
}

pub fn setup_ability_machine_definition(mut commands: Commands) {
    commands.insert_resource(build_machine());
}

struct ReadyState;
impl MachineState<AbilityMachine> for ReadyState {
    fn on_enter(&self, _ctx: &mut Access<AbilityMachine>) {}

    fn on_event(&self, ctx: &mut Access<AbilityMachine>, event: &AbilityEvent) -> EventResult {
        debug!("on_event: {:?}", event);
        match event {
            AbilityEvent::TryActivate {
                source,
                target: target_data,
            } => {
                let Ok((ability, ability_ref)) = ctx.view.abilities.get(ctx.ability_id) else {
                    return EventResult::Ignored;
                };

                let Ok((_, source_entity_ref, _actor_abilities)) = ctx.view.actors.get(*source)
                else {
                    warn!(
                        "[{}] The Actor({}) has no GrantedAbilities",
                        ctx.ability_id, source
                    );
                    return EventResult::Ignored;
                };

                let target_entity_ref = match target_data {
                    TargetData::SelfCast => Some(source_entity_ref),
                    TargetData::Target(target) => {
                        let Ok((_, entity, _)) = ctx.view.actors.get(*target) else {
                            return EventResult::Ignored;
                        };
                        Some(entity)
                    }
                    _ => None,
                };

                let ability_spec = ctx
                    .view
                    .registry
                    .ability_assets
                    .get(&ability.0.clone())
                    .ok_or("No ability asset.")
                    .unwrap();

                // Checks if we meet the Actor-based conditions (i.e. is not stunned)
                let context = ActorExprContext {
                    actor_context: source_entity_ref,
                };
                let meets_actor_exec_conditions = ability_spec
                    .execution_conditions
                    .iter()
                    .all(|expr| expr.eval(&context));

                if !meets_actor_exec_conditions {
                    return EventResult::Ignored;
                }

                let context = AbilityExprContext {
                    caster_ref: source_entity_ref,
                    ability_ref,
                    target_ref: target_entity_ref,
                };

                let can_activate =
                    can_activate_ability(&context, &ability_spec, &BoolExpr::new(|_ctx| true))
                        .ok()
                        .unwrap_or(false);

                if can_activate {
                    ctx.internal_events.push_back(AbilityEvent::Activate);

                    ctx.view.commands.trigger(BeginAbility {
                        source: *source,
                        ability: ctx.ability_id,
                    });
                    ctx.view.commands.trigger(ActivateAbility {
                        target: target_data.clone(),
                        source: *source,
                        ability: ctx.ability_id,
                    });
                    ctx.view.commands.trigger(ExecuteAbility {
                        target: target_data.clone(),
                        source: *source,
                        ability: ctx.ability_id,
                    });
                }
                EventResult::Handled
            }

            _ => EventResult::Ignored,
        }
    }
}

struct ActiveState;
impl MachineState<AbilityMachine> for ActiveState {
    fn on_enter(&self, ctx: &mut Access<AbilityMachine>) {
        debug!("[{}] Ability enter ActiveState", ctx.ability_id);
        let Ok(tasks) = ctx.view.tasks.get(ctx.ability_id) else {
            error!("Activated an unavailable ability.");
            return;
        };

        // Abilities without tasks auto complete
        if tasks.is_empty() {
            ctx.internal_events.push_back(AbilityEvent::EndAbility);
            return;
        }

        // Signal tasks to begin
        for task_entity in tasks.iter() {
            let _ = ctx
                .view
                .task_machines
                .dispatch_event(task_entity, TaskEvent::Activate);
        }
    }

    fn on_exit(&self, ctx: &mut Access<AbilityMachine>) {
        debug!("[{}] Ability exit ActiveState", ctx.ability_id);
    }
}

struct RecoveryState;
impl MachineState<AbilityMachine> for RecoveryState {
    fn on_enter(&self, ctx: &mut Access<AbilityMachine>) {
        debug!("on_enter: CooldownState");

        // Reset recovery elapsed timer
        ctx.data.recovery_timer.set_base_value(0.0);

        // Sets the recovery timer cooldown. Uses a snapshotting model.
        ctx.data.timers.set_timer(
            ctx.data.cooldown.val(),
            AbilityEvent::Recovered,
            AbilityState::Recovery,
        );

        for task_id in ctx.view.tasks.iter_descendants(ctx.ability_id) {
            if ctx
                .view
                .task_machines
                .is_in_state(task_id, TaskState::Running)
            {
                ctx.view
                    .task_machines
                    .dispatch_event(task_id, TaskEvent::Stop)
                    .unwrap();
            }
        }
    }

    fn on_exit(&self, ctx: &mut Access<AbilityMachine>) {
        for task_id in ctx.view.tasks.iter_descendants(ctx.ability_id) {
            ctx.view
                .task_machines
                .dispatch_event(task_id, TaskEvent::Reset)
                .unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_mermaid_state_machine() {
        let machine = build_machine();
        println!("{}", machine.to_mermaid().unwrap());
    }
}
