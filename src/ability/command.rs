use crate::ability::Ability;
use crate::ability::ability_state::AbilityMachine;
use crate::assets::AbilityDef;
use bevy::asset::{Assets, Handle};
use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;
use hfsm_bevy::MachineInstance;

pub struct GrantAbilityCommand {
    pub parent: Entity,
    pub handle: Handle<AbilityDef>,
}

impl EntityCommand for GrantAbilityCommand {
    type Out = ();

    fn apply(self, mut actor: EntityWorldMut) -> () {
        let ability_def = {
            // Create a temporary scope to borrow the world
            let world = actor.world();
            let actor_assets = world.resource::<Assets<AbilityDef>>();
            actor_assets.get(&self.handle).unwrap()
        }; // World borrow ends here

        let mut queue = {
            let mut queue = CommandQueue::default();
            let mut commands = Commands::new(&mut queue, actor.world());

            // Apply mutators
            for mutator in &ability_def.mutators {
                let mut entity_commands = commands.entity(actor.id());
                mutator.apply(&mut entity_commands);
            }

            for observer in &ability_def.observers {
                let mut entity_commands = commands.entity(self.parent);
                observer.apply(&mut entity_commands);
            }

            queue
        };

        let scene = (ability_def.scene)();

        let _ = actor
            .insert((
                Ability(self.handle),
                Name::new(ability_def.name.clone()),
                MachineInstance::<AbilityMachine>::default(),
            ))
            .apply_scene(scene);

        // Apply the commands
        actor.world_scope(|world| {
            world.commands().append(&mut queue);
            world.flush();
        });
    }
}
