use crate::mutator::EntityActions;
use bevy::ecs::query::{QueryData, QueryItem};
use bevy::ecs::system::{StaticSystemParam, SystemParam, SystemParamItem};
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;

#[derive(EntityEvent)]
pub struct BeginTask {
    #[event_target]
    pub entity: Entity,
}

#[derive(EntityEvent)]
pub struct CancelTask {
    #[event_target]
    pub entity: Entity,
}

#[derive(EntityEvent)]
pub struct EndTask {
    #[event_target]
    pub entity: Entity,
}

#[derive(EntityEvent)]
pub struct TaskCompleted {
    #[event_target]
    pub entity: Entity,
}

pub type TaskItem<'w, 's, T> = QueryItem<'w, 's, <T as AbilityTask>::Query>;
pub type TaskParam<'w, 's, T> = SystemParamItem<'w, 's, <T as AbilityTask>::Param>;

pub trait AbilityTask: Send + Sync + 'static {
    /// The query descriptor. (e.g. `&'static mut Health` or a struct deriving `QueryData`)
    type Query: QueryData + Send + Sync + 'static;
    type Param: SystemParam + Send + Sync + 'static;

    fn on_begin(_query: TaskItem<Self>, _param: &mut TaskParam<Self>) {}
    fn on_cancel(query: TaskItem<Self>) {
        Self::on_end(query);
    }
    fn on_completion(_query: TaskItem<Self>) {}
    fn on_end(_query: TaskItem<Self>) {}
}

pub fn task<T: AbilityTask>() -> impl Scene {
    bsn! {
        on(|trigger: On<BeginTask>,
         mut query: Query<T::Query>,
         params: StaticSystemParam<T::Param>| {
            let item = query.get_mut(trigger.event_target()).unwrap();
            let mut param_items = params.into_inner();
            T::on_begin(item, &mut param_items);
        })
        on(|trigger: On<CancelTask>, mut query: Query<T::Query>| {
            let item = query.get_mut(trigger.event_target()).unwrap();
            T::on_cancel(item);
        })
        on(|trigger: On<TaskCompleted>, mut query: Query<T::Query>| {
            let item = query.get_mut(trigger.event_target()).unwrap();
            T::on_completion(item);
        })
        on(|trigger: On<EndTask>, mut query: Query<T::Query>| {
            let item = query.get_mut(trigger.event_target()).unwrap();
            T::on_end(item);
        })
    }
}

/// The entity that this effect is targeting.
#[derive(Component, Reflect, Debug)]
#[relationship(relationship_target = Tasks)]
pub struct TaskOwner(pub Entity);

/// All abilities granted to this entity.
#[derive(Component, Reflect, Debug, Default)]
#[relationship_target(relationship = TaskOwner, linked_spawn)]
pub struct Tasks(Vec<Entity>);

pub struct DebugTask;
impl AbilityTask for DebugTask {
    type Query = DebugTaskContext;
    type Param = ();

    fn on_begin(query: TaskItem<Self>, _param: &mut TaskParam<Self>) {
        debug!("[{}] Task Begin", query.name);
    }

    fn on_cancel(query: TaskItem<Self>) {
        debug!("[{}] Task Cancelled", query.name);
    }

    fn on_completion(query: TaskItem<Self>) {
        debug!("[{}] Task Completed", query.name);
    }

    fn on_end(query: TaskItem<Self>) {
        debug!("[{}] Task Ended", query.name);
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
pub struct DebugTaskContext {
    entity: Entity,
    name: Read<Name>,
}