use crate::AttributesRef;
use crate::ability::systems::can_activate_ability;
use crate::ability::task_states::{TaskEvent, TaskMachine};
use crate::ability::tasks::{BeginTask, Tasks};
use crate::ability::{Ability, AbilityCooldown, GrantedAbilities, TargetData};
use crate::actors::Actor;
use crate::registry::Registry;
use bevy::ecs::query::QueryData;
use bevy::ecs::resource::IsResource;
use bevy::ecs::system::SystemParam;
use bevy::ecs::system::lifetimeless::{Read, Write};
use bevy::log::{error, warn};
use bevy::prelude::{
    AppTypeRegistry, Commands, Entity, Query, RelationshipTarget, Res, Without, debug,
};
use express_it::logic::BoolExpr;
use hfsm_bevy::{
    Access, EventResult, ExternalContext, LocalContext, Machine, MachineDefinition, MachineQuery,
    MachineState, StateId,
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
    //timers: Write<StateTimer<AbilityMachine>>,
}
impl LocalContext for AbilityContext {
    type Item<'w, 's> = <Self as QueryData>::Item<'w, 's>;
}

#[derive(SystemParam)]
pub struct AbilitySystemParam<'w, 's> {
    abilities: Query<
        'w,
        's,
        (
            Read<Ability>,
            AttributesRef<'static, 'static>,
            Read<Tasks>,
            Option<Read<AbilityCooldown>>,
        ),
        Without<IsResource>,
    >,
    actors: Query<
        'w,
        's,
        (
            Read<Actor>,
            AttributesRef<'static, 'static>,
            Read<GrantedAbilities>,
        ),
        Without<IsResource>,
    >,
    tasks: Query<'w, 's, Read<Tasks>>,
    task_machines: MachineQuery<'w, 's, TaskMachine>,
    registry: Registry<'w>,
    type_registry: Res<'w, AppTypeRegistry>,
    commands: Commands<'w, 's>,
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
}

fn build_machine() -> MachineDefinition<AbilityMachine> {
    MachineDefinition::<AbilityMachine>::builder(AbilityState::Ready, |root| {
        root.leaf(AbilityState::Ready, "Ready", ReadyState)
            .on(AbilityEvent::Activate, AbilityState::Active);

        root.leaf(AbilityState::Active, "Active", ActiveState)
            .on(AbilityEvent::EndAbility, AbilityState::Recovery)
            .on(AbilityEvent::CancelAbility, AbilityState::Recovery)
            .then(AbilityState::Recovery);

        root.leaf(AbilityState::Recovery, "Cooldown", CooldownState)
            .then(AbilityState::Ready);
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
    fn on_enter(&self, _ctx: &mut Access<AbilityMachine>) {
        println!("on_enter: ReadyState");
    }

    fn on_event(&self, ctx: &mut Access<AbilityMachine>, event: &AbilityEvent) -> EventResult {
        debug!("on_event: {:?}", event);
        match event {
            AbilityEvent::TryActivate { source, target } => {
                let Ok((ability, ability_ref, _, opt_cooldown)) =
                    ctx.view.abilities.get(ctx.ability_id)
                else {
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

                let target_entity_ref = match target {
                    TargetData::SelfCast => source_entity_ref,
                    TargetData::Target(target) => {
                        let Ok((_, entity, _)) = ctx.view.actors.get(*target) else {
                            return EventResult::Ignored;
                        };
                        entity
                    }
                };

                // Handle cooldowns
                let is_finished = match opt_cooldown {
                    None => true,
                    Some(cd) => cd.timer.is_finished(),
                };
                if !is_finished {
                    return EventResult::Ignored;
                }

                // Get the ability spec from assets
                let ability_spec = ctx
                    .view
                    .registry
                    .ability_assets
                    .get(&ability.0.clone())
                    .ok_or("No ability asset.")
                    .unwrap();

                let can_activate = can_activate_ability(
                    &ability_ref,
                    &source_entity_ref,
                    &target_entity_ref,
                    &ability_spec,
                    &BoolExpr::true_(),
                    &ctx.view.type_registry.0.clone(),
                )
                .ok()
                .unwrap_or(false);

                if can_activate {
                    ctx.internal_events.push_back(AbilityEvent::Activate);
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
        let Ok((ability, ability_ref, tasks, _)) = ctx.view.abilities.get(ctx.ability_id) else {
            error!("Activated an unavailable ability.");
            return;
        };

        // Spawn tasks if they exist
        let ability_def = ctx
            .view
            .registry
            .ability_assets
            .get(&ability.0.clone())
            .ok_or("No ability asset.")
            .unwrap();

        if tasks.is_empty() {
            ctx.internal_events.push_back(AbilityEvent::EndAbility);
            return;
        }

        // Signal tasks to begin
        for task_entity in tasks.iter() {
            ctx.view.commands.trigger(BeginTask {
                task_id: task_entity,
            });
        }
    }

    fn on_exit(&self, ctx: &mut Access<AbilityMachine>) {
        debug!("[{}] Ability exit ActiveState", ctx.ability_id);
    }
}

struct CooldownState;
impl MachineState<AbilityMachine> for CooldownState {
    fn on_exit(&self, ctx: &mut Access<AbilityMachine>) {
        for task_id in ctx.view.tasks.iter_descendants(ctx.ability_id) {}
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
