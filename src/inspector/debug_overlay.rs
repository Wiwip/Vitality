use crate::ability::ability_state::AbilityMachine;
use crate::ability::tasks::Tasks;
use crate::ability::{Ability, AbilityCooldown, GrantedAbilities};
use crate::actors::Actor;
use crate::attributes::ReflectAccessAttribute;
use crate::effect::{AppliedEffects, Effect, Stacks};
use crate::modifier::{ModifierMarker, OwnedModifiers, ReflectAccessModifier};
use crate::prelude::*;
use bevy::ecs::component::{ComponentId, Components};
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use ptree::{TreeBuilder, write_tree};
use std::any::TypeId;

#[derive(Component, Default, Copy, Clone)]
pub struct DebugOverlayMarker;

pub fn setup_debug_overlay(mut commands: Commands) {
    commands.spawn_scene(create_overlay());
}

fn create_overlay() -> impl Scene {
    println!("Creating overlay");
    bsn! {
        #DebugOverlay
        Node {
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::FlexStart,
            margin: UiRect::axes(Val::Px(5.), Val::Px(5.)),
        }
        Children [
            Text("Debug Overlay")
            TextFont {
                font: FontSourceTemplate::Handle("fonts/JetBrainsMono-Regular.ttf"),
                font_size: px(12.0),
            }
            DebugOverlayMarker,
        ]
    }
}

pub fn explore_actors_system(
    actors: Query<(EntityRef, Option<&AppliedEffects>), (With<Actor>, With<DebugOverlayMarker>)>,
    effects: Query<(&Effect, &Stacks, Option<&Name>, Option<&OwnedModifiers>)>,
    granted_abilities: Query<&GrantedAbilities>,
    ability_query: Query<(&Ability, Option<&AbilityCooldown>, Option<&Name>)>,
    tasks: Query<&Tasks>,
    names: Query<&Name>,
    modifiers: Query<EntityRef, With<ModifierMarker>>,
    type_registry: Res<AppTypeRegistry>,
    world_components: &Components,
    mut debug_overlay: Query<
        &mut Text,
        (
            With<DebugOverlayMarker>,
            Without<Actor>,
            Without<ModifierMarker>,
        ),
    >,
) {
    let mut builder = TreeBuilder::new("Actor Tree".into());

    for (actor_ref, actor_effects) in actors.iter() {
        builder.begin_child(format!("Actor {}", actor_ref.id()));

        let mut actor_components = get_components_sorted(world_components, actor_ref);

        list_attributes(
            &mut builder,
            &type_registry,
            actor_ref,
            &mut actor_components,
        );

        if let Some(actor_effects) = actor_effects {
            list_effects(
                &mut builder,
                actor_effects,
                effects,
                modifiers,
                &type_registry,
                &world_components,
            );
        }

        if let Ok(granted_abilities) = granted_abilities.get(actor_ref.id()) {
            list_abilities(
                &mut builder,
                granted_abilities,
                &ability_query,
                &tasks,
                &names,
            )
        }

        builder.end_child();
    }

    let tree = builder.build();
    if let Ok(mut text) = debug_overlay.single_mut() {
        let mut w = Vec::new();
        let _ = write_tree(&tree, &mut w);
        text.0 = String::from_utf8(w).unwrap();
    } else {
        warn_once!("Debug overlay rendering failed");
    }
}

fn list_attributes(
    builder: &mut TreeBuilder,
    type_registry: &AppTypeRegistry,
    actor_ref: EntityRef,
    actor_components: &mut Vec<(String, ComponentId, Option<TypeId>, usize)>,
) {
    builder.begin_child("Attributes".to_string());
    // List attributes for printing
    for (_, _, type_id, _) in actor_components.iter() {
        let Some(type_id) = type_id else {
            continue;
        };

        let registry = type_registry.read();
        let reflect_attribute = registry.get_type_data::<ReflectAccessAttribute>(*type_id);
        let Some(reflect_access_attribute) = reflect_attribute else {
            continue;
        };

        let Some(type_registration) = registry.get(*type_id) else {
            error!("Failed to get type registration for entity {:?}", *type_id);
            continue;
        };
        let Some(reflect_component) = type_registration.data::<ReflectComponent>() else {
            error!("No reflect access attribute found");
            continue;
        };
        let Some(dyn_reflect) = reflect_component.reflect(actor_ref) else {
            error!("Failed to reflect entity");
            continue;
        };

        let Some(attribute) = reflect_access_attribute.get(dyn_reflect) else {
            continue;
        };

        builder
            .begin_child(format!(
                "{}: {:.1}",
                attribute.name(),
                attribute.access_current_value()
            ))
            .end_child();
    }
    builder.end_child();
}

fn list_effects(
    mut builder: &mut TreeBuilder,
    actor_effects: &AppliedEffects,
    effect_query: Query<(&Effect, &Stacks, Option<&Name>, Option<&OwnedModifiers>)>,
    modifier_query: Query<EntityRef, With<ModifierMarker>>,
    type_registry: &AppTypeRegistry,
    world_components: &Components,
) {
    builder.begin_child("Effects".to_string());
    for effect_entity in actor_effects.iter() {
        let Ok((_, stacks, name, modifiers)) = effect_query.get(effect_entity) else {
            continue;
        };
        let name = match name {
            None => "Effect",
            Some(name) => name,
        };

        builder.begin_child(format!(
            "[{effect_entity}] {name} [{}]",
            stacks.current_value()
        ));

        let Some(modifiers) = modifiers else {
            continue;
        };
        for modifier in modifiers.iter() {
            let Ok(modifier_ref) = modifier_query.get(modifier) else {
                continue;
            };

            let mut modifier_components = get_components_sorted(world_components, modifier_ref);

            list_modifiers_for_effect(
                &mut builder,
                type_registry,
                modifier_ref,
                &mut modifier_components,
            );
        }
        builder.end_child();
    }
    builder.end_child();
}

fn list_tasks(
    builder: &mut TreeBuilder,
    tasks: &Tasks,
    names: &Query<&Name>,
    task_query: &Query<&Tasks>,
) {
    for task_entity in tasks.iter() {
        let name = names.get(task_entity).map(|n| n.as_str()).unwrap_or("Task");

        if let Ok(tasks) = task_query.get(task_entity) {
            builder.begin_child(format!("[{task_entity}] {name}"));
            list_tasks(builder, tasks, names, task_query);
            builder.end_child();
        } else {
            builder
                .begin_child(format!("[{task_entity}] {name}"))
                .end_child();
        }
    }
}

fn list_abilities(
    builder: &mut TreeBuilder,
    abilities: &GrantedAbilities,
    ability_query: &Query<(&Ability, Option<&AbilityCooldown>, Option<&Name>)>,
    task_query: &Query<&Tasks>,
    names: &Query<&Name>,
) {
    builder.begin_child("Abilities".to_string());
    for ability_entity in abilities.iter() {
        let Ok((_ability, cooldown, name)) = ability_query.get(*ability_entity) else {
            continue;
        };

        let name = name.map(|n| n.as_str()).unwrap_or("Ability");
        let cooldown_str = if let Some(cooldown) = cooldown {
            format!(" ({:.1}s)", cooldown.timer.remaining_secs())
        } else {
            "".to_string()
        };

        builder.begin_child(format!("[{ability_entity}] {name}{cooldown_str}"));

        if let Ok(tasks) = task_query.get(*ability_entity) {
            list_tasks(builder, tasks, names, task_query);
        }

        builder.end_child();
    }
    builder.end_child();
}

fn get_components_sorted(
    world_components: &Components,
    modifier_ref: EntityRef,
) -> Vec<(String, ComponentId, Option<TypeId>, usize)> {
    let archetype = modifier_ref.archetype();
    let mut modifier_components: Vec<_> = archetype
        .components()
        .into_iter()
        .map(|component_id| {
            let info = world_components.get_info(*component_id).unwrap();
            let name = info.name().to_string();

            (name, *component_id, info.type_id(), info.layout().size())
        })
        .collect();
    modifier_components.sort_by(|(name_a, ..), (name_b, ..)| name_a.cmp(name_b));
    modifier_components
}

fn list_modifiers_for_effect(
    builder: &mut TreeBuilder,
    type_registry: &AppTypeRegistry,
    modifier_ref: EntityRef,
    modifier_components: &mut Vec<(String, ComponentId, Option<TypeId>, usize)>,
) {
    // List attributes for printing
    for (name, _, type_id, _) in modifier_components.iter() {
        let Some(type_id) = type_id else {
            error!(
                "Failed to get type registration for entity {:?} / {}",
                *type_id, name
            );
            continue;
        };

        let registry = type_registry.read();
        let reflect_attribute = registry.get_type_data::<ReflectAccessModifier>(*type_id);
        let Some(reflect_access_modifier) = reflect_attribute else {
            // No reflect access modifier found, but normal since not all components are an AttributeModifier
            continue;
        };

        let Some(type_registration) = registry.get(*type_id) else {
            error!("Failed to get type registration for entity {:?}", *type_id);
            continue;
        };
        let Some(reflect_component) = type_registration.data::<ReflectComponent>() else {
            error!("No reflect access attribute found");
            continue;
        };
        let Some(dyn_reflect) = reflect_component.reflect(modifier_ref) else {
            error!("Failed to reflect entity");
            continue;
        };

        let Some(modifier) = reflect_access_modifier.get(dyn_reflect) else {
            continue;
        };

        builder
            .begin_child(format!("[{}] {}", modifier_ref.id(), modifier.describe(),))
            .end_child();
    }
}
