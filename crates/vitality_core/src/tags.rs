use crate::util::pretty_type_name;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use std::any::TypeId;

/// A tag component erased so effect and ability definitions can store it.
#[derive(Clone, Copy, Debug)]
pub struct TagSpec {
    pub type_id: TypeId,
    pub name: fn() -> String,
    insert: fn(&mut EntityWorldMut),
    remove: fn(&mut EntityWorldMut),
}

impl PartialEq for TagSpec {
    fn eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id
    }
}

impl TagSpec {
    pub fn of<T: Component + Default>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            name: pretty_type_name::<T>,
            insert: |entity| {
                entity.insert(T::default());
            },
            remove: |entity| {
                entity.remove::<T>();
            },
        }
    }

    pub fn insert_on(&self, entity: &mut EntityWorldMut) {
        (self.insert)(entity);
    }
}

/// how many grants hold each tag, so overlapping sources don't remove it early; the tag component itself sits on the actor only while the count is above zero
#[derive(Component, Default, Debug)]
pub struct ActorTags {
    counts: HashMap<TypeId, u32>,
}

impl ActorTags {
    pub fn count(&self, type_id: TypeId) -> u32 {
        self.counts.get(&type_id).copied().unwrap_or(0)
    }
}

/// Grants or revokes one counted tag. Queue it with `commands.entity(actor).queue(...)`.
#[derive(Clone, Copy)]
pub struct ChangeTag {
    pub spec: TagSpec,
    pub grant: bool,
}

impl ChangeTag {
    pub fn grant(spec: TagSpec) -> Self {
        Self { spec, grant: true }
    }

    pub fn revoke(spec: TagSpec) -> Self {
        Self { spec, grant: false }
    }
}

impl EntityCommand for ChangeTag {
    type Out = ();

    fn apply(self, mut entity: EntityWorldMut) {
        let mut tags = entity.take::<ActorTags>().unwrap_or_default();
        let id = self.spec.type_id;
        if self.grant {
            let count = tags.counts.entry(id).or_default();
            *count += 1;
            if *count == 1 {
                (self.spec.insert)(&mut entity);
            }
        } else if let Some(count) = tags.counts.get_mut(&id) {
            *count -= 1;
            if *count == 0 {
                tags.counts.remove(&id);
                (self.spec.remove)(&mut entity);
            }
        }
        entity.insert(tags);
    }
}
