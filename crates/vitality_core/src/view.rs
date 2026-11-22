use bevy_ecs::bundle::Bundle;
use bevy_ecs::component::{Component, Mutable};
use bevy_ecs::prelude::{Entity, Mut};
use bevy_ecs::world::{EntityMutExcept, EntityRefExcept, FilteredEntityMut, FilteredEntityRef};
use std::any::TypeId;

/// each crate picks its own exclusion list, so the view stays local to the crate owning those components
pub trait ActorRead {
    fn id(&self) -> Entity;
    fn get<C: Component>(&self) -> Option<&C>;
    fn has_type(&self, type_id: TypeId) -> bool;
}

pub trait ActorWrite: ActorRead {
    fn get_mut<C: Component<Mutability = Mutable>>(&mut self) -> Option<Mut<'_, C>>;
}

impl<B: Bundle> ActorRead for EntityRefExcept<'_, '_, B> {
    fn id(&self) -> Entity {
        EntityRefExcept::id(self)
    }
    // the query resolved the bundle's ids into `access` once; the inherent `get` looks each up again per read
    fn get<C: Component>(&self) -> Option<&C> {
        FilteredEntityRef::from(self).get::<C>()
    }
    fn has_type(&self, type_id: TypeId) -> bool {
        EntityRefExcept::contains_type_id(self, type_id)
    }
}

impl<B: Bundle> ActorRead for EntityMutExcept<'_, '_, B> {
    fn id(&self) -> Entity {
        EntityMutExcept::id(self)
    }
    fn get<C: Component>(&self) -> Option<&C> {
        EntityMutExcept::get(self)
    }
    fn has_type(&self, type_id: TypeId) -> bool {
        EntityMutExcept::contains_type_id(self, type_id)
    }
}

impl<B: Bundle> ActorWrite for EntityMutExcept<'_, '_, B> {
    fn get_mut<C: Component<Mutability = Mutable>>(&mut self) -> Option<Mut<'_, C>> {
        EntityMutExcept::get_mut(self)
    }
}

impl ActorRead for FilteredEntityRef<'_, '_> {
    fn id(&self) -> Entity {
        FilteredEntityRef::id(self)
    }
    fn get<C: Component>(&self) -> Option<&C> {
        FilteredEntityRef::get(self)
    }
    fn has_type(&self, type_id: TypeId) -> bool {
        FilteredEntityRef::contains_type_id(self, type_id)
    }
}

impl ActorRead for FilteredEntityMut<'_, '_> {
    fn id(&self) -> Entity {
        FilteredEntityMut::id(self)
    }
    fn get<C: Component>(&self) -> Option<&C> {
        FilteredEntityMut::get(self)
    }
    fn has_type(&self, type_id: TypeId) -> bool {
        FilteredEntityMut::contains_type_id(self, type_id)
    }
}

impl ActorWrite for FilteredEntityMut<'_, '_> {
    fn get_mut<C: Component<Mutability = Mutable>>(&mut self) -> Option<Mut<'_, C>> {
        FilteredEntityMut::get_mut(self)
    }
}
