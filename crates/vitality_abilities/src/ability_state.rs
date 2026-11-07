use crate::systems::{ActivateAbility, can_activate_ability};
use bevy::log::{error, warn};
use bevy::prelude::{RelationshipTarget, debug};
use express_it::expr::BoolExpr;
use hfsm_bevy::{Access, EventResult, MachineDefinition, MachineState};
use std::time::Duration;
use vitality_core::ability::ability_state::{AbilityEvent, AbilityMachine, AbilityState};
use vitality_core::ability::task_states::{TaskEvent, TaskState};
use vitality_core::ability::{BeginAbility, ExecuteAbility, TargetData};
use vitality_core::attributes::Attribute;
use vitality_core::context::{AbilityExprContext, ActorExprContext};

pub(crate) fn build_machine() -> MachineDefinition<AbilityMachine> {
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
                let Ok((ability, ability_ref)) = ctx.external.abilities.get(ctx.ability_id) else {
                    return EventResult::Ignored;
                };

                let Ok((source_entity_ref, _actor_abilities)) = ctx.external.actors.get(*source)
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
                        let Ok((entity, _)) = ctx.external.actors.get(*target) else {
                            return EventResult::Ignored;
                        };
                        Some(entity)
                    }
                    _ => None,
                };

                let ability_spec = ctx
                    .external
                    .registry
                    .ability_assets
                    .get(&ability.handle.clone())
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

                    ctx.external.commands.trigger(BeginAbility {
                        source: *source,
                        ability: ctx.ability_id,
                    });
                    ctx.external.commands.trigger(ActivateAbility {
                        target: target_data.clone(),
                        source: *source,
                        ability: ctx.ability_id,
                    });
                    ctx.external.commands.trigger(ExecuteAbility {
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
        let Ok(tasks) = ctx.external.tasks.get(ctx.ability_id) else {
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
            if let Err(err) = ctx
                .external
                .task_machines
                .dispatch_event(task_entity, TaskEvent::Activate)
            {
                error!("[{}] TaskMachine dispatch failed: {err}", task_entity);
            }
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
        ctx.local.recovery_timer.set_base_value(0.0);

        let (ability, ability_ref) = ctx.external.abilities.get(ctx.ability_id).unwrap();
        let ability_def = ctx.external.ability_assets.get(&ability.handle).unwrap();

        let owner_id = ctx.external.casters.get(ctx.ability_id).unwrap();
        let (caster_ref, _) = ctx.external.actors.get(owner_id.0).unwrap();

        let actor_context = AbilityExprContext {
            caster_ref,
            target_ref: None,
            ability_ref,
        };
        let new_cooldown = ability_def.cooldown.eval(&actor_context);

        // Sets the recovery timer cooldown. Uses a snapshotting model.
        ctx.local.timers.set_timer(
            Duration::from_secs_f32(new_cooldown),
            AbilityEvent::Recovered,
            AbilityState::Recovery,
        );

        for task_id in ctx.external.tasks.iter_descendants(ctx.ability_id) {
            if ctx
                .external
                .task_machines
                .is_in_state(task_id, TaskState::Running)
                .inspect_err(|err| error!("[{}] TaskMachine is_in_state failed: {err}", task_id))
                .unwrap_or_default()
            {
                if let Err(err) = ctx
                    .external
                    .task_machines
                    .dispatch_event(task_id, TaskEvent::Stop)
                {
                    error!("[{}] TaskMachine dispatch failed: {err}", task_id);
                }
            }
        }
    }

    fn on_exit(&self, ctx: &mut Access<AbilityMachine>) {
        for task_id in ctx.external.tasks.iter_descendants(ctx.ability_id) {
            if let Err(err) = ctx
                .external
                .task_machines
                .dispatch_event(task_id, TaskEvent::Reset)
            {
                error!("[{}] TaskMachine dispatch failed: {err}", task_id);
            }
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
