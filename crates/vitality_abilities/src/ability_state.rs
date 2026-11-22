use crate::AbilityRef;
use crate::context::AbilityExprContext;
use crate::definition::AbilityDef;
use crate::lifecycle::{
    AbilityActivated, AbilityActivationFailed, AbilityEnded, AbilityEnding, ActivationError,
    change_blocks, check_activation,
};
use crate::registry::Registry;
use crate::systems::ActivateAbility;
use crate::task_states::TaskMachine;
use crate::task_states::{TaskEvent, TaskState};
use crate::tasks::{StartComposite, Tasks};
use crate::{Ability, AbilityOf, GrantedAbilities, TargetData};
use crate::{BeginAbility, ExecuteAbility};
use bevy_asset::Assets;
use bevy_ecs::prelude::{Commands, Entity, Query, RelationshipTarget, Res, Without};
use bevy_ecs::query::QueryData;
use bevy_ecs::resource::IsResource;
use bevy_ecs::system::SystemParam;
use bevy_ecs::system::lifetimeless::{Read, Write};
use bevy_log::{debug, error, warn};
use hfsm_bevy::{Access, EventResult, MachineDefinition, MachineEvent, MachineState};
use hfsm_bevy::{ExternalContext, LocalContext, Machine, MachineQuery, StateId, StateTimer};
use std::time::Duration;
use vitality_core::tags::ChangeTag;
use vitality_core::view::ActorRead;

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
    pub ability_id: Entity,
    pub timers: Write<StateTimer<AbilityMachine>>,
    pub ending: Write<AbilityEnding>,
}
impl LocalContext for AbilityContext {
    type Item<'w, 's> = <Self as QueryData>::Item<'w, 's>;
}

#[derive(SystemParam)]
pub struct AbilitySystemParam<'w, 's> {
    pub abilities:
        Query<'w, 's, (Read<Ability>, AbilityRef<'static, 'static>), Without<IsResource>>,
    pub actors:
        Query<'w, 's, (AbilityRef<'static, 'static>, Read<GrantedAbilities>), Without<IsResource>>,
    pub casters: Query<'w, 's, Read<AbilityOf>>,
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
    TryActivate {
        source: Entity,
        target: TargetData,
    },
    Activate,
    CancelAbility,
    EndAbility,
    /// The root's `OnFail::Propagate` saw a child fail.
    FailAbility,
    TimerExpired,
    Recovered,
}

pub(crate) fn build_machine() -> MachineDefinition<AbilityMachine> {
    MachineDefinition::<AbilityMachine>::builder(AbilityState::Ready, |root| {
        root.leaf(AbilityState::Ready, "Ready", ReadyState)
            .on(AbilityEvent::Activate, AbilityState::Active);

        root.leaf(AbilityState::Active, "Active", ActiveState)
            .on(AbilityEvent::EndAbility, AbilityState::Recovery)
            .on(AbilityEvent::CancelAbility, AbilityState::Recovery)
            .on(AbilityEvent::FailAbility, AbilityState::Recovery);

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
        let AbilityEvent::TryActivate {
            source,
            target: target_data,
        } = event
        else {
            return EventResult::Ignored;
        };
        let (source, target_data) = (*source, *target_data);
        let ability_id = ctx.ability_id;

        let Ok((ability, ability_ref)) = ctx.external.abilities.get(ability_id) else {
            return EventResult::Ignored;
        };
        let Ok((caster_ref, _)) = ctx.external.actors.get(source) else {
            warn!("[{ability_id}] The Actor({source}) has no GrantedAbilities");
            return EventResult::Ignored;
        };
        let target_ref = match target_data {
            TargetData::SelfCast => Some(caster_ref),
            TargetData::Target(target) => {
                let Ok((target_ref, _)) = ctx.external.actors.get(target) else {
                    ctx.external.commands.trigger(AbilityActivationFailed {
                        entity: ability_id,
                        caster: source,
                        reason: ActivationError::NoSuchTarget,
                    });
                    return EventResult::Handled;
                };
                Some(target_ref)
            }
            TargetData::Location(_) => None,
        };
        let Some(def) = ctx.external.registry.ability_assets.get(&ability.handle) else {
            warn!(
                "[{ability_id}] not activated: its handle holds no AbilityDef in Assets<AbilityDef>; \
                 load the asset before triggering the ability"
            );
            return EventResult::Ignored;
        };

        if let Err(reason) = check_activation(def, &caster_ref, &ability_ref, target_ref) {
            ctx.external.commands.trigger(AbilityActivationFailed {
                entity: ability_id,
                caster: source,
                reason,
            });
            return EventResult::Handled;
        }

        // cancel before granting our own blocks, so a cancel-and-replace pair works
        if !def.cancels_abilities_with.is_empty()
            && let Ok((_, granted)) = ctx.external.actors.get(source)
        {
            for other in granted.iter().copied().filter(|other| *other != ability_id) {
                let Ok((_, other_ref)) = ctx.external.abilities.get(other) else {
                    continue;
                };
                if def
                    .cancels_abilities_with
                    .iter()
                    .any(|tag| other_ref.has_type(tag.type_id))
                {
                    ctx.external
                        .commands
                        .trigger(MachineEvent::<AbilityMachine>::new(
                            other,
                            AbilityEvent::CancelAbility,
                        ));
                }
            }
        }

        let mut caster = ctx.external.commands.entity(source);
        for tag in &def.activation_owned_tags {
            caster.queue_silenced(ChangeTag::grant(*tag));
        }
        change_blocks(
            &mut caster,
            def.blocks_abilities_with
                .iter()
                .map(|tag| tag.type_id)
                .collect(),
            true,
        );

        ctx.internal_events.push_back(AbilityEvent::Activate);
        ctx.external.commands.trigger(AbilityActivated {
            entity: ability_id,
            caster: source,
        });
        ctx.external.commands.trigger(BeginAbility {
            source,
            ability: ability_id,
        });
        ctx.external.commands.trigger(ActivateAbility {
            target: target_data,
            source,
            ability: ability_id,
        });
        ctx.external.commands.trigger(ExecuteAbility {
            target: target_data,
            source,
            ability: ability_id,
        });
        EventResult::Handled
    }
}

fn report_busy(ctx: &mut Access<AbilityMachine>, event: &AbilityEvent, reason: ActivationError) {
    if let AbilityEvent::TryActivate { source, .. } = event {
        ctx.external.commands.trigger(AbilityActivationFailed {
            entity: ctx.ability_id,
            caster: *source,
            reason,
        });
    }
}

struct ActiveState;
impl MachineState<AbilityMachine> for ActiveState {
    fn on_enter(&self, ctx: &mut Access<AbilityMachine>) {
        debug!("[{}] Ability enter ActiveState", ctx.ability_id);
        ctx.local.ending.cancelled = false;
        ctx.local.ending.failed = false;
        let Ok(tasks) = ctx.external.tasks.get(ctx.ability_id) else {
            error!("Activated an unavailable ability.");
            return;
        };

        if tasks.is_empty() {
            ctx.internal_events.push_back(AbilityEvent::EndAbility);
            return;
        }

        // the ability is the root composite, started like any other
        ctx.external.commands.trigger(StartComposite {
            node: ctx.ability_id,
        });
    }

    fn on_event(&self, ctx: &mut Access<AbilityMachine>, event: &AbilityEvent) -> EventResult {
        match event {
            AbilityEvent::CancelAbility => ctx.local.ending.cancelled = true,
            AbilityEvent::FailAbility => ctx.local.ending.failed = true,
            AbilityEvent::TryActivate { .. } => {
                report_busy(ctx, event, ActivationError::AlreadyActive);
                return EventResult::Handled;
            }
            _ => {}
        }
        EventResult::Ignored
    }

    fn on_exit(&self, ctx: &mut Access<AbilityMachine>) {
        debug!("[{}] Ability exit ActiveState", ctx.ability_id);
        let ability_id = ctx.ability_id;
        let Ok(caster) = ctx.external.casters.get(ability_id).map(|of| of.0) else {
            return;
        };
        if let Ok((ability, _)) = ctx.external.abilities.get(ability_id)
            && let Some(def) = ctx.external.ability_assets.get(&ability.handle)
            && let Ok(mut caster_commands) = ctx.external.commands.get_entity(caster)
        {
            for tag in &def.activation_owned_tags {
                caster_commands.queue_silenced(ChangeTag::revoke(*tag));
            }
            change_blocks(
                &mut caster_commands,
                def.blocks_abilities_with
                    .iter()
                    .map(|tag| tag.type_id)
                    .collect(),
                false,
            );
        }
        ctx.external.commands.trigger(AbilityEnded {
            entity: ability_id,
            caster,
            cancelled: ctx.local.ending.cancelled,
            failed: ctx.local.ending.failed,
        });
    }
}

struct RecoveryState;
impl MachineState<AbilityMachine> for RecoveryState {
    fn on_enter(&self, ctx: &mut Access<AbilityMachine>) {
        debug!("on_enter: CooldownState");

        let ability_id = ctx.ability_id;
        let cooldown = (|| {
            let (ability, ability_ref) = ctx.external.abilities.get(ability_id).ok()?;
            let ability_def = ctx.external.ability_assets.get(&ability.handle)?;
            let owner_id = ctx.external.casters.get(ability_id).ok()?;
            let (caster_ref, _) = ctx.external.actors.get(owner_id.0).ok()?;
            let actor_context = AbilityExprContext {
                caster_ref,
                target_ref: None,
                ability_ref,
                location: None,
            };
            Some(ability_def.cooldown.eval(&actor_context))
        })();
        let Some(cooldown) = cooldown else {
            warn!("[{ability_id}] Could not evaluate the cooldown; recovering immediately.");
            ctx.internal_events.push_back(AbilityEvent::Recovered);
            return;
        };

        // non-finite or negative cooldowns would panic in Duration
        let cooldown = if cooldown.is_finite() {
            cooldown.max(0.0)
        } else {
            0.0
        };
        if cooldown == 0.0 {
            ctx.internal_events.push_back(AbilityEvent::Recovered);
        } else {
            ctx.local.timers.set_timer(
                Duration::from_secs_f32(cooldown),
                AbilityEvent::Recovered,
                AbilityState::Recovery,
            );
        }

        // only the direct children: a stopped composite stops its own
        let children: Vec<Entity> = ctx
            .external
            .tasks
            .get(ability_id)
            .map(|tasks| tasks.iter().collect())
            .unwrap_or_default();
        for task_id in children {
            if ctx
                .external
                .task_machines
                .is_in_state(task_id, TaskState::Running)
                .inspect_err(|err| error!("[{}] TaskMachine is_in_state failed: {err}", task_id))
                .unwrap_or_default()
                && let Err(err) = ctx
                    .external
                    .task_machines
                    .dispatch_event(task_id, TaskEvent::Stop)
            {
                error!("[{}] TaskMachine dispatch failed: {err}", task_id);
            }
        }
    }

    fn on_event(&self, ctx: &mut Access<AbilityMachine>, event: &AbilityEvent) -> EventResult {
        if matches!(event, AbilityEvent::TryActivate { .. }) {
            report_busy(ctx, event, ActivationError::OnCooldown);
            return EventResult::Handled;
        }
        EventResult::Ignored
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
