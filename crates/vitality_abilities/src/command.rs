use crate::definition::AbilityDef;
use crate::{Ability, AbilityOf};
use bevy_asset::Assets;
use bevy_ecs::prelude::*;
use bevy_log::warn;
use bevy_scene::EntityCommandsSceneExt;

pub fn on_add_ability(
    trigger: On<Add<Ability>>,
    abilities: Query<(&Ability, &AbilityOf)>,
    ability_assets: Res<Assets<AbilityDef>>,
    mut commands: Commands,
) {
    let Ok((ability, _)) = abilities.get(trigger.entity) else {
        warn!("[{}] Ability added without an owner.", trigger.entity);
        return;
    };
    let Some(ability_def) = ability_assets.get(&ability.handle) else {
        warn!(
            "[{}] Ability not added: its handle holds no AbilityDef in Assets<AbilityDef>; \
             load the asset before adding the ability",
            trigger.entity
        );
        return;
    };

    let mut ability_entity_commands = commands.entity(trigger.entity);
    // the overlay reads the row label off the entity, like it does for actors and effects
    ability_entity_commands.insert(Name::new(ability_def.name.clone()));
    for mutator in &ability_def.mutators {
        mutator.apply(&mut ability_entity_commands);
    }
    // a trigger hangs off the ability entity, which is what revoking despawns
    for actions in &ability_def.observers {
        actions.apply(&mut ability_entity_commands);
    }

    let scene = (ability_def.task_scene)();
    commands.entity(trigger.entity).apply_scene(scene);
}
