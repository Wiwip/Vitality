use crate::ability::Ability;
use crate::assets::AbilityDef;
use crate::modifier::modifier::RecalculateExpression;
use bevy::asset::{Assets, Handle};
use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

pub struct GrantAbilityCommand {
    pub parent: Entity,
    pub handle: Handle<AbilityDef>,
}

impl EntityCommand for GrantAbilityCommand {
    type Out = ();

    fn apply(self, mut ability_id: EntityWorldMut) -> () {
        let ability_def = {
            // Create a temporary scope to borrow the world
            let world = ability_id.world();
            let actor_assets = world.resource::<Assets<AbilityDef>>();
            actor_assets.get(&self.handle).unwrap()
        }; // World borrow ends here

        let mut queue = {
            let mut queue = CommandQueue::default();
            let mut commands = Commands::new(&mut queue, ability_id.world());

            // Apply mutators
            for mutator in &ability_def.mutators {
                let mut entity_commands = commands.entity(ability_id.id());
                mutator.apply(&mut entity_commands);
            }

            for observer in &ability_def.observers {
                let mut entity_commands = commands.entity(self.parent);
                observer.apply(&mut entity_commands);
            }

            queue
        };

        let scene = (ability_def.task_scene)();

        let recovery_observer = |_trigger: On<RecalculateExpression>| {
            println!("recalculate expression");
        };

        let _ = ability_id
            .insert((
                Ability(self.handle),
                Name::new(ability_def.name.clone()),
            ))
            .observe(recovery_observer)
            .apply_scene(scene);

        // Apply the commands
        ability_id.world_scope(|world| {
            world.commands().append(&mut queue);
            world.flush();
        });
    }
}
