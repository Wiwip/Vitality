use crate::AttributesRef;
use crate::ability::systems::can_activate_ability;
use crate::ability::{Abilities, Ability, AbilityCooldown, GrantedAbilities, TargetData};
use crate::actors::Actor;
use crate::registry::{Registry, RegistryMut};
use bevy::ecs::query::QueryData;
use bevy::ecs::resource::IsResource;
use bevy::ecs::system::SystemParam;
use bevy::ecs::system::lifetimeless::{Read, Write};
use bevy::log::warn;
use bevy::prelude::{AppTypeRegistry, Commands, Entity, Query, Res, Time, Without};
use express_it::logic::BoolExpr;
use hfsm_bevy::{
    Access, EventResult, ExternalContext, LocalContext, Machine, MachineDefinition, MachineState,
    StateId, StateTimer,
};

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AbilityState {
    Root,
    Ready,
    Active,
    Cooldown,
}
impl From<AbilityState> for StateId {
    fn from(value: AbilityState) -> Self {
        Self::try_from(value as u16).unwrap()
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
pub struct AbilityContext {
    ability_entity: Entity,
    timers: Write<StateTimer<AbilityMachine>>,
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
    registry: Registry<'w>,
    type_registry: Res<'w, AppTypeRegistry>,
    time: Res<'w, Time>,
}
impl ExternalContext for AbilitySystemParam<'static, 'static> {
    type Item<'w, 's> = AbilitySystemParam<'w, 's>;
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum AbilityEvent {
    TryActivate { source: Entity, target: TargetData },
    CancelAbility,
    EndAbility,
    TimerExpired,
}

pub struct AbilityMachine;
impl Machine for AbilityMachine {
    type Local = AbilityContext;
    type External = AbilitySystemParam<'static, 'static>;
    type Event = AbilityEvent;
}

fn build_machine() -> MachineDefinition<AbilityMachine> {
    MachineDefinition::<AbilityMachine>::builder(AbilityState::Ready, |root| {
        root.leaf(AbilityState::Ready, "Ready", ReadyState);
        root.leaf(AbilityState::Active, "Active", ActiveState);
    })
    .build()
    .expect("Failed to build HFSM")
    .into()
}

pub fn setup_machine_definition(mut commands: Commands) {
    commands.insert_resource(build_machine());
}

struct ReadyState;
impl MachineState<AbilityMachine> for ReadyState {
    fn on_enter(&self, _ctx: &mut Access<AbilityMachine>) {}

    fn on_event(&self, ctx: &mut Access<AbilityMachine>, event: &AbilityEvent) -> EventResult {
        match event {
            AbilityEvent::TryActivate { source, target } => {
                let Ok((ability, ability_ref, _cooldown)) =
                    ctx.view.abilities.get(ctx.ability_entity)
                else {
                    return EventResult::Ignored;
                };

                let Ok((_, source_entity_ref, _actor_abilities)) = ctx.view.actors.get(*source)
                else {
                    warn!(
                        "[{}] The Actor({}) has no GrantedAbilities",
                        ctx.ability_entity, source
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
                    EventResult::Transition(AbilityState::Active.into())
                } else {
                    EventResult::Handled
                }
            }

            _ => EventResult::Ignored,
        }
    }
}

struct ActiveState;
impl MachineState<AbilityMachine> for ActiveState {
    fn on_enter(&self, _ctx: &mut Access<AbilityMachine>) {
        println!("Enter: ActiveState");
    }

    fn on_exit(&self, _ctx: &mut Access<AbilityMachine>) {
        println!("Exit: ActiveState.")
    }
}

struct CooldownState;
impl MachineState<AbilityMachine> for CooldownState {}
