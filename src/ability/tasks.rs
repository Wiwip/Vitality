use bevy::ecs::query::{QueryData, QueryItem};
use bevy::ecs::system::{SystemParam, SystemParamItem};
use bevy::prelude::{Entity, EntityEvent, Query};

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
    fn on_end(_query: TaskItem<Self>) {}
}

/*#[derive(SystemParam)]
pub struct AbilityTaskQuery<'w, 's, T>
where
    T: AbilityTask,
{
    pub query: Query<'w, 's, (Entity, <T as AbilityTask>::Query)>,
}
*/
