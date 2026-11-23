use bevy_ecs::component::{ComponentId, Components};
use bevy_ecs::prelude::*;
use bevy_ecs::reflect::AppTypeRegistry;
use bevy_ecs::system::SystemParam;
use bevy_log::{error, warn_once};
use bevy_scene::{CommandsSceneExt, Scene, bsn};
use bevy_text::TextFont;
use bevy_ui::{prelude::*, px};
use ptree::print_config::{ASCII_CHARS_TICK, PrintConfig};
use ptree::{TreeBuilder, write_tree_with};
use std::any::TypeId;
use vitality_abilities::ability_state::AbilityMachine;
use vitality_abilities::tasks::{Composite, Tasks};
use vitality_abilities::{Ability, GrantedAbilities, StateTimer, cooldown_remaining};
use vitality_core::attributes::ManageAttributes;
use vitality_core::attributes::ReflectAccessAttribute;
use vitality_effects::modifier::{ModifierMarker, OwnedModifiers, ReflectAccessModifier};
use vitality_effects::prelude::*;
use vitality_effects::{AppliedEffects, Effect, Stacks};

#[derive(Component, Default, Copy, Clone)]
pub struct DebugOverlayMarker;

type ActorQuery<'w, 's> = Query<
    'w,
    's,
    (
        EntityRef<'static>,
        Option<&'static AppliedEffects>,
        Option<&'static Name>,
    ),
    // the overlay's own text node carries the marker too, and is what OverlayQuery writes
    (With<DebugOverlayMarker>, Without<Text>),
>;

type AbilityQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Ability,
        Option<&'static StateTimer<AbilityMachine>>,
        Option<&'static Name>,
    ),
>;

type OverlayQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        With<DebugOverlayMarker>,
        Without<ManageAttributes>,
        Without<ModifierMarker>,
    ),
>;

pub fn setup_debug_overlay(mut commands: Commands) {
    commands.spawn_scene(create_overlay());
}

fn create_overlay() -> impl Scene {
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
            // bevy's default font: the repo's ttf is git-ignored and was missing on fresh clones
            TextFont {
                font_size: px(12.0),
            }
            DebugOverlayMarker
        ]
    }
}

#[derive(SystemParam)]
pub struct ExploreParams<'w, 's> {
    effects: Query<
        'w,
        's,
        (
            &'static Effect,
            &'static Stacks,
            Option<&'static Name>,
            Option<&'static OwnedModifiers>,
        ),
    >,
    granted_abilities: Query<'w, 's, &'static GrantedAbilities>,
    ability_query: AbilityQuery<'w, 's>,
    tasks: Query<'w, 's, TaskRow>,
    names: Query<'w, 's, &'static Name>,
    modifiers: Query<'w, 's, EntityRef<'static>, With<ModifierMarker>>,
    type_registry: Res<'w, AppTypeRegistry>,
}

// bevy's default font has no box-drawing glyphs, so ptree's utf branches draw as tofu
fn tree_config() -> PrintConfig {
    PrintConfig {
        characters: ASCII_CHARS_TICK.into(),
        ..Default::default()
    }
}

pub fn explore_actors_system(
    actors: ActorQuery<'_, '_>,
    ctx: ExploreParams<'_, '_>,
    world_components: &Components,
    mut debug_overlay: OverlayQuery<'_, '_>,
) {
    let mut builder = TreeBuilder::new("Actor Tree".into());

    for (actor_ref, actor_effects, actor_name) in actors.iter() {
        builder.begin_child(actor_row(actor_name.map(Name::as_str), actor_ref.id()));

        let mut actor_components = get_components_sorted(world_components, actor_ref);

        list_attributes(
            &mut builder,
            &ctx.type_registry,
            actor_ref,
            &mut actor_components,
        );

        if let Some(actor_effects) = actor_effects {
            list_effects(
                &mut builder,
                actor_effects,
                &ctx.effects,
                &ctx.modifiers,
                &ctx.type_registry,
                world_components,
            );
        }

        if let Ok(granted_abilities) = ctx.granted_abilities.get(actor_ref.id()) {
            list_abilities(
                &mut builder,
                granted_abilities,
                &ctx.ability_query,
                &ctx.tasks,
                &ctx.names,
            )
        }

        builder.end_child();
    }

    let tree = builder.build();
    if let Ok(mut text) = debug_overlay.single_mut() {
        let mut w = Vec::new();
        let _ = write_tree_with(&tree, &mut w, &tree_config());
        let rendered = String::from_utf8(w).unwrap();
        // assigning Text unconditionally change-detects it and bevy re-lays-out the block
        if text.0 != rendered {
            text.0 = rendered;
        }
    } else {
        warn_once!("Debug overlay rendering failed");
    }
}

fn list_attributes(
    builder: &mut TreeBuilder,
    type_registry: &AppTypeRegistry,
    actor_ref: EntityRef,
    actor_components: &mut [(String, ComponentId, Option<TypeId>, usize)],
) {
    builder.begin_child("Attributes".to_string());
    for (name, _, type_id, _) in actor_components.iter() {
        let Some(type_id) = type_id else {
            continue;
        };

        let registry = type_registry.read();
        let reflect_attribute = registry.get_type_data::<ReflectAccessAttribute>(*type_id);
        let Some(reflect_access_attribute) = reflect_attribute else {
            continue;
        };

        let Some(type_registration) = registry.get(*type_id) else {
            error!(
                "attribute walk: no type registration for {:?} / {}",
                *type_id, name
            );
            continue;
        };
        let Some(reflect_component) = type_registration.data::<ReflectComponent>() else {
            error!(
                "attribute walk: no ReflectComponent for {:?} / {}",
                *type_id, name
            );
            continue;
        };
        let Some(dyn_reflect) = reflect_component.reflect(actor_ref) else {
            error!(
                "attribute walk: failed to reflect {:?} / {}",
                *type_id, name
            );
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
    builder: &mut TreeBuilder,
    actor_effects: &AppliedEffects,
    effect_query: &Query<(&Effect, &Stacks, Option<&Name>, Option<&OwnedModifiers>)>,
    modifier_query: &Query<EntityRef, With<ModifierMarker>>,
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

        if let Some(modifiers) = modifiers {
            for modifier in modifiers.iter() {
                let Ok(modifier_ref) = modifier_query.get(modifier) else {
                    continue;
                };

                let mut modifier_components = get_components_sorted(world_components, modifier_ref);

                list_modifiers_for_effect(
                    builder,
                    type_registry,
                    modifier_ref,
                    &mut modifier_components,
                );
            }
        }
        builder.end_child();
    }
    builder.end_child();
}

type TaskRow = (&'static Tasks, Option<&'static Composite>);

fn list_tasks(
    builder: &mut TreeBuilder,
    tasks: &Tasks,
    names: &Query<&Name>,
    task_query: &Query<TaskRow>,
) {
    for task_entity in tasks.iter() {
        let name = names.get(task_entity).map(|n| n.as_str()).unwrap_or("Task");

        if let Ok((tasks, composite)) = task_query.get(task_entity) {
            let kind = composite
                .map(|kind| format!(" ({kind:?})"))
                .unwrap_or_default();
            builder.begin_child(format!("[{task_entity}] {name}{kind}"));
            list_tasks(builder, tasks, names, task_query);
            builder.end_child();
        } else {
            builder
                .begin_child(format!("[{task_entity}] {name}"))
                .end_child();
        }
    }
}

fn actor_row(name: Option<&str>, entity: Entity) -> String {
    match name {
        Some(name) => format!("Actor {name} ({entity})"),
        None => format!("Actor {entity}"),
    }
}

fn cooldown_column(timer: Option<&StateTimer<AbilityMachine>>) -> String {
    match timer.and_then(cooldown_remaining) {
        Some(remaining) => format!(" ({:.1}s)", remaining.as_secs_f32()),
        None => "".to_string(),
    }
}

fn list_abilities(
    builder: &mut TreeBuilder,
    abilities: &GrantedAbilities,
    ability_query: &AbilityQuery<'_, '_>,
    task_query: &Query<TaskRow>,
    names: &Query<&Name>,
) {
    builder.begin_child("Abilities".to_string());
    for ability_entity in abilities.iter() {
        let Ok((_, timer, name)) = ability_query.get(*ability_entity) else {
            continue;
        };

        let name = name.map(|n| n.as_str()).unwrap_or("Ability");
        let cooldown_str = cooldown_column(timer);

        builder.begin_child(format!("[{ability_entity}] {name}{cooldown_str}"));

        if let Ok((tasks, _)) = task_query.get(*ability_entity) {
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
        .iter()
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
    modifier_components: &mut [(String, ComponentId, Option<TypeId>, usize)],
) {
    for (name, _, type_id, _) in modifier_components.iter() {
        let Some(type_id) = type_id else {
            continue;
        };

        let registry = type_registry.read();
        let reflect_attribute = registry.get_type_data::<ReflectAccessModifier>(*type_id);
        let Some(reflect_access_modifier) = reflect_attribute else {
            // No reflect access modifier found, but normal since not all components are an AttributeModifier
            continue;
        };

        let Some(type_registration) = registry.get(*type_id) else {
            error!(
                "modifier walk: no type registration for {:?} / {}",
                *type_id, name
            );
            continue;
        };
        let Some(reflect_component) = type_registration.data::<ReflectComponent>() else {
            error!(
                "modifier walk: no ReflectComponent for {:?} / {}",
                *type_id, name
            );
            continue;
        };
        let Some(dyn_reflect) = reflect_component.reflect(modifier_ref) else {
            error!("modifier walk: failed to reflect {:?} / {}", *type_id, name);
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use std::time::Duration;
    use vitality_abilities::AbilityOf;
    use vitality_abilities::ability_state::{AbilityEvent, AbilityState};
    use vitality_effects::EffectTarget;
    use vitality_effects::modifier::ModifierOf;

    fn recovering(seconds: f32) -> StateTimer<AbilityMachine> {
        let mut timer = StateTimer::<AbilityMachine>::default();
        timer.set_timer(
            Duration::from_secs_f32(seconds),
            AbilityEvent::Recovered,
            AbilityState::Recovery,
        );
        timer
    }

    #[test]
    fn an_ability_that_never_activated_shows_no_cooldown() {
        assert_eq!(cooldown_column(None), "");
    }

    #[test]
    fn a_cooldown_shows_the_time_left() {
        let timer = recovering(0.5);
        assert_eq!(cooldown_column(Some(&timer)), " (0.5s)");
    }

    #[test]
    fn an_actor_row_names_the_actor_and_keeps_the_entity() {
        let entity = Entity::from_raw_u32(25).unwrap();
        let row = actor_row(Some("Hero"), entity);
        assert!(row.contains("Hero"), "{row}");
        assert!(row.contains(&entity.to_string()), "{row}");
    }

    #[test]
    fn a_nameless_actor_row_is_the_entity_alone() {
        let entity = Entity::from_raw_u32(25).unwrap();
        assert_eq!(actor_row(None, entity), format!("Actor {entity}"));
    }

    #[test]
    fn a_modifier_less_effect_closes_its_row() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_resource::<AppTypeRegistry>()
            .add_systems(Update, explore_actors_system);

        let world = app.world_mut();
        let actor = world.spawn((ManageAttributes, DebugOverlayMarker)).id();
        let buffed = world
            .spawn((
                Effect(vitality_core::store::Handle::default()),
                EffectTarget(actor),
                Name::new("Buffed"),
            ))
            .id();
        world.spawn((ModifierOf(buffed), ModifierMarker));
        world.spawn((
            Effect(vitality_core::store::Handle::default()),
            EffectTarget(actor),
            Name::new("Stunned"),
        ));
        world.spawn((
            Ability {
                handle: Handle::default(),
            },
            AbilityOf(actor),
            Name::new("Fireball"),
        ));
        let overlay = world.spawn((Text::default(), DebugOverlayMarker)).id();

        app.update();

        let rendered = app.world().get::<Text>(overlay).unwrap().0.clone();

        assert_eq!(
            indent(&rendered, "Abilities"),
            indent(&rendered, "Effects"),
            "the ability column must sit beside the effect column:\n{rendered}"
        );
        assert_eq!(
            indent(&rendered, "Buffed"),
            indent(&rendered, "Stunned"),
            "both effect rows must sit at the same depth:\n{rendered}"
        );
    }

    #[test]
    fn a_marked_entity_that_is_not_an_actor_renders_its_effects() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_resource::<AppTypeRegistry>()
            .add_systems(Update, explore_actors_system);

        let world = app.world_mut();
        let post = world.spawn(DebugOverlayMarker).id();
        world.spawn((
            Effect(vitality_core::store::Handle::default()),
            EffectTarget(post),
            Name::new("Warded"),
        ));
        let overlay = world.spawn((Text::default(), DebugOverlayMarker)).id();

        app.update();

        let rendered = app.world().get::<Text>(overlay).unwrap().0.clone();
        assert!(
            rendered.contains(&post.to_string()) && rendered.contains("Warded"),
            "the marked non-actor must get a row with its effect listed:\n{rendered}"
        );
        assert!(
            !rendered.contains(&overlay.to_string()),
            "the overlay renders itself, it is not a row of its own:\n{rendered}"
        );
    }

    // ptree pads every level to a fixed width, so the offset of the label is the depth
    fn indent(rendered: &str, needle: &str) -> usize {
        rendered
            .lines()
            .find(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("{needle} is missing from:\n{rendered}"))
            .chars()
            .take_while(|c| !c.is_alphanumeric() && *c != '[')
            .count()
    }
}
