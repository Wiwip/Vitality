use crate::view::{ActorRead, ActorWrite};
use bevy::ecs::world::{FilteredEntityMut, FilteredEntityRef};
use express_it::expr::{Context, ContextMut};

pub struct Source;
pub struct Target;
pub struct Caster;

// A trait for contexts that have a "Source" or primary actor
pub trait ActorProvider<'a, 'b, Role> {
    type ActorRef: ActorRead;
    fn get_actor(&self) -> &Self::ActorRef;
}
pub trait ActorProviderMut<'a, 'b, Role> {
    type ActorMut: ActorWrite;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut;
}

/*********************************************************
 * Actor
 *********************************************************/
pub struct ActorExprSchema;
impl Context for ActorExprSchema {
    type ContextItem<'w, 's> = ActorExprContext<'w, 's>;
}
impl ContextMut for ActorExprSchema {
    type ContextItemMut<'w, 's> = ActorExprContextMut<'w, 's>;

    fn as_readonly<'w, 's, 'a>(
        mut_item: &'a Self::ContextItemMut<'w, 's>,
    ) -> Self::ContextItem<'a, 's> {
        ActorExprContext {
            actor_context: mut_item.actor_context.as_readonly(),
        }
    }
}

pub struct ActorExprContext<'w, 's> {
    pub actor_context: FilteredEntityRef<'w, 's>,
}
pub struct ActorExprContextMut<'w, 's> {
    pub actor_context: FilteredEntityMut<'w, 's>,
}

impl<'w, 's> ActorProvider<'w, 's, Source> for ActorExprContext<'w, 's> {
    type ActorRef = FilteredEntityRef<'w, 's>;
    fn get_actor(&self) -> &Self::ActorRef {
        &self.actor_context
    }
}
impl<'w, 's> ActorProviderMut<'w, 's, Source> for ActorExprContextMut<'w, 's> {
    type ActorMut = FilteredEntityMut<'w, 's>;
    fn get_actor_mut(&mut self) -> &mut Self::ActorMut {
        &mut self.actor_context
    }
}
