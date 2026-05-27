use crate::condition::systems::evaluate_effect_conditions;
use bevy::app::{App, Plugin};
use bevy::prelude::*;

mod conditions;
mod systems;

use crate::schedule::EffectsSet;
pub use conditions::{ChanceCondition, HasComponent, IsAbility, IsAttributeWithinBounds};

pub struct ConditionPlugin;

impl Plugin for ConditionPlugin {
    fn build(&self, app: &mut App) {
        // This system is responsible for checking conditions and
        // activating/deactivating their related effects.
        app.add_systems(
            Update,
            evaluate_effect_conditions.in_set(EffectsSet::Prepare),
        );
        //app.add_systems(Update, evaluate_effect_conditions.in_set(EffectsSet::Notify));
    }
}

/*
#[cfg(test)]
mod test {
    use super::*;

    use std::marker::PhantomData;

    attribute!(Test1, f32);
    attribute!(Test2, f32);

    #[test]
    fn test() {
        let mut world = World::new();
        world.spawn((Test1::new(100.0), Test2::new(100.0)));

        let _ = world.run_system_once(|actor: Single<AttributesRef>| {
            let ctx = BevyContext {
                source_actor: &actor,
                target_actor: &actor,
                owner: &actor,

            };
        });
    }
}
*/
