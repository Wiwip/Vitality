//! Append-only definition store replacing `bevy_asset`, so handles are plain indices stable across runs.

use bevy_ecs::resource::Resource;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Handle<T> {
    index: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    pub fn id(&self) -> u32 {
        self.index
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Handle<T> {}
// points at nothing
impl<T> Default for Handle<T> {
    fn default() -> Self {
        Self {
            index: u32::MAX,
            marker: PhantomData,
        }
    }
}
impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<T> Eq for Handle<T> {}
impl<T> PartialOrd for Handle<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Handle<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.index.cmp(&other.index)
    }
}
impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}
impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle({})", self.index)
    }
}

#[derive(Resource)]
pub struct Assets<T: Send + Sync + 'static> {
    items: Vec<T>,
    /// a definition is out of sync with whatever was derived from it at `add` time
    revised: AtomicBool,
}

impl<T: Send + Sync + 'static> Default for Assets<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            revised: AtomicBool::new(false),
        }
    }
}

impl<T: Send + Sync + 'static> Assets<T> {
    pub fn add(&mut self, item: T) -> Handle<T> {
        let index = self.items.len() as u32;
        self.items.push(item);
        Handle {
            index,
            marker: PhantomData,
        }
    }

    pub fn get(&self, handle: &Handle<T>) -> Option<&T> {
        self.items.get(handle.index as usize)
    }

    /// the borrow can rewrite a definition live consumers already derived from, hence [`Self::take_revised`]
    pub fn get_mut(&mut self, handle: &Handle<T>) -> Option<&mut T> {
        self.revised.store(true, Ordering::Relaxed);
        self.items.get_mut(handle.index as usize)
    }

    /// true once per run of [`Self::get_mut`], so a `Res` reader can re-derive without `ResMut`
    pub fn take_revised(&self) -> bool {
        self.revised.swap(false, Ordering::Relaxed)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
