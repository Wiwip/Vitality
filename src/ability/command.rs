use crate::ability::{Ability, AbilityOf};
use crate::assets::AbilityDef;
use bevy::asset::Assets;
use bevy::prelude::*;

/*pub struct GrantAbilityCommand {
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

        let _ = ability_id
            .insert((
                Ability {
                    handle: self.handle,
                },
                Name::new(ability_def.name.clone()),
                AbilityOf(self.parent),
            ))
            .apply_scene(scene);

        // Apply the commands
        ability_id.world_scope(|world| {
            world.commands().append(&mut queue);
            world.flush();
        });
    }
}*/

pub fn on_add_ability(
    trigger: On<Add, Ability>,
    abilities: Query<(&Ability, &AbilityOf)>,
    ability_assets: Res<Assets<AbilityDef>>,
    mut commands: Commands,
) {
    let (ability, ability_of) = abilities.get(trigger.entity).unwrap();
    let ability_def = ability_assets.get(&ability.handle).unwrap();

    let mut ability_entity_commands = commands.entity(trigger.entity);
    for mutator in &ability_def.mutators {
        mutator.apply(&mut ability_entity_commands);
    }

    let mut parent_entity_commands = commands.entity(ability_of.0);
    for observer in &ability_def.observers {
        observer.apply(&mut parent_entity_commands);
    }

    let scene = (ability_def.task_scene)();
    commands.entity(trigger.entity).apply_scene(scene);
}
