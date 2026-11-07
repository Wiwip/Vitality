use bevy::asset::Assets;
use bevy::prelude::*;
use vitality_core::ability::{Ability, AbilityOf};
use vitality_core::assets::AbilityDef;

pub fn on_add_ability(
    trigger: On<Add<Ability>>,
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
