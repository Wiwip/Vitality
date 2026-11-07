use bevy::ecs::bundle::Bundle;
use bevy::ecs::component::{Component, Mutable};
use bevy::ecs::world::{EntityMutExcept, EntityRefExcept};
use bevy::prelude::{Entity, Mut};

/// Read access to one entity's attributes. Each crate picks its own exclusion list, so the
/// view type stays local to the crate that owns the components it excludes.
pub trait ActorRead {
    fn id(&self) -> Entity;
    fn get<C: Component>(&self) -> Option<&C>;
}

pub trait ActorWrite: ActorRead {
    fn get_mut<C: Component<Mutability = Mutable>>(&mut self) -> Option<Mut<'_, C>>;
}

impl<B: Bundle> ActorRead for EntityRefExcept<'_, '_, B> {
    fn id(&self) -> Entity {
        EntityRefExcept::id(self)
    }
    fn get<C: Component>(&self) -> Option<&C> {
        EntityRefExcept::get(self)
    }
}

impl<B: Bundle> ActorRead for EntityMutExcept<'_, '_, B> {
    fn id(&self) -> Entity {
        EntityMutExcept::id(self)
    }
    fn get<C: Component>(&self) -> Option<&C> {
        EntityMutExcept::get(self)
    }
}

impl<B: Bundle> ActorWrite for EntityMutExcept<'_, '_, B> {
    fn get_mut<C: Component<Mutability = Mutable>>(&mut self) -> Option<Mut<'_, C>> {
        EntityMutExcept::get_mut(self)
    }
}
