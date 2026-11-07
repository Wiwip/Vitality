use crate::AbilityRef;
use crate::ability::task_states::TaskMachine;
use crate::ability::tasks::Tasks;
use crate::ability::{Ability, AbilityOf, AbilityRecovery, GrantedAbilities, TargetData};
use crate::assets::AbilityDef;
use crate::registry::Registry;
use bevy::asset::Assets;
use bevy::ecs::query::QueryData;
use bevy::ecs::resource::IsResource;
use bevy::ecs::system::SystemParam;
use bevy::ecs::system::lifetimeless::{Read, Write};
use bevy::prelude::{Commands, Entity, Query, Res, Without};
use hfsm_bevy::{ExternalContext, LocalContext, Machine, MachineQuery, StateId, StateTimer};

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
    pub recovery_timer: Write<AbilityRecovery>,
    pub timers: Write<StateTimer<AbilityMachine>>,
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
    TryActivate { source: Entity, target: TargetData },
    Activate,
    CancelAbility,
    EndAbility,
    TimerExpired,
    Recovered,
}
