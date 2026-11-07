use bevy::asset::Assets;
use bevy::ecs::resource::IsResource;
use bevy::log::debug;
use bevy::prelude::*;
use vitality_core::assets::EffectDef;
use vitality_core::context::EffectExprContext;
use vitality_core::effect::{
    AppliedEffects, ApplyEffectEvent, Effect, EffectApplicationPolicy, EffectSource,
    EffectStackingPolicy, EffectTarget, NotifyAddStackEvent,
};
use vitality_core::graph::NodeType;
use vitality_core::modifier::ModifierOf;
use vitality_core::modifier::modifier::Modifier;
use vitality_core::{AppAttributeBindings, EffectsMut};

fn apply_instant_effect(
    event: &ApplyEffectEvent,
    actors: &mut Query<
        (Option<&AppliedEffects>, EffectsMut),
        (Without<Effect>, Without<IsResource>),
    >,
    commands: &mut Commands,
    effect: &EffectDef,
) -> Result<(), BevyError> {
    debug!("Applying instant effect to {}", event.targeting.target());

    let Ok((_, source_actor_ref)) = actors.get(event.targeting.source()) else {
        warn!("Failed to get source actor: {}", event.targeting.source());
        return Ok(());
    };
    let Ok((_, target_actor_ref)) = actors.get(event.targeting.target()) else {
        warn!("Failed to get target actor: {}", event.targeting.target());
        return Ok(());
    };

    let context = EffectExprContext {
        target_actor: Some(target_actor_ref),
        source_actor: source_actor_ref,
        effect_holder: source_actor_ref, // TODO: Make optional
    };

    // Determines whether the effect should activate
    let should_apply = effect
        .activate_conditions
        .iter()
        .all(|condition| condition.eval(&context));

    if !should_apply {
        debug!("Effect not applied.");
        return Ok(());
    }

    apply_modifiers(event, actors, &mut effect.modifiers.iter(), commands);
    Ok(())
}

fn apply_modifiers<'a, I>(
    event: &ApplyEffectEvent,
    _actors: &'a mut Query<
        (Option<&AppliedEffects>, EffectsMut),
        (Without<Effect>, Without<IsResource>),
    >,
    modifiers: &mut I,
    commands: &mut Commands,
) where
    I: Iterator<Item = &'a Box<dyn Modifier>>,
{
    for modifier in modifiers {
        modifier.apply_delayed(
            event.targeting.source(),
            event.targeting.target(),
            event.entity,
            commands,
        );
    }
}

fn spawn_persistent_effect(
    event: &ApplyEffectEvent,
    commands: &mut Commands,
    effect: &EffectDef,
    actors: &mut Query<
        (Option<&AppliedEffects>, EffectsMut),
        (Without<Effect>, Without<IsResource>),
    >,
    effects: &mut Query<&Effect>,
    add_stack_event: &mut MessageWriter<NotifyAddStackEvent>,
    type_bindings: AppAttributeBindings,
) -> Result<(), BevyError> {
    // We want to know whether an effect with the same handle already points to the actor
    let (optional_effects, _) = actors.get_mut(event.targeting.target())?;
    let effects_on_actor = match optional_effects {
        None => {
            vec![]
        }
        Some(effects_on_actor) => {
            let effects = effects_on_actor.iter().filter_map(|effect_entity| {
                let Ok(other_effect) = effects.get(effect_entity) else {
                    return None;
                };
                if other_effect.0.id() == event.handle.id() {
                    Some(effect_entity)
                } else {
                    None
                }
            });
            effects.collect::<Vec<_>>()
        }
    };

    match effect.stacking_policy {
        EffectStackingPolicy::None => {
            // Continue spawning effect
        }
        EffectStackingPolicy::Add { .. } | EffectStackingPolicy::RefreshDuration => {
            if effects_on_actor.len() > 0 {
                debug!("Effect already exists on actor. Adding stacks per definition.");
                add_stack_event.write(NotifyAddStackEvent {
                    effect_entity: *effects_on_actor.first().unwrap(),
                    handle: event.handle.clone(),
                });
                return Ok(());
            }
        }
    }

    let (_, source_actor_ref) = actors.get(event.targeting.source())?;
    let (_, target_actor_ref) = actors.get(event.targeting.target())?;

    let context = EffectExprContext {
        target_actor: Some(target_actor_ref),
        source_actor: source_actor_ref,
        effect_holder: source_actor_ref, // TODO: Should this be the source actor? The effect doesn't exist for instant effects.
    };

    // Determines whether the effect should activate
    let should_be_applied = effect
        .attach_conditions
        .iter()
        .all(|condition| condition.eval(&context));

    if !should_be_applied {
        return Ok(());
    }

    let mut effect_commands = commands.spawn_empty();
    let effect_entity = effect_commands.id();
    for effect_fn in &effect.effect_fn {
        effect_fn(&mut effect_commands, event.targeting.target());
    }

    // Spawns the effect entity
    effect_commands.insert((
        NodeType::Effect,
        EffectTarget(event.targeting.target()),
        EffectSource(event.targeting.source()),
        Effect(event.handle.clone()),
    ));

    // Converts the policy to components that can be added to the entity
    let (duration, ticker) = effect.application_policy.to_bundles();
    if let Some(duration) = duration {
        effect_commands.insert(duration);
    }
    if let Some(ticker) = ticker {
        effect_commands.insert(ticker);
    }

    // Spawn effect modifiers
    let bindings = type_bindings.internal.read().unwrap();
    effect.modifiers.iter().for_each(|modifier| {
        let mut entity_commands = commands.spawn(ModifierOf(effect_entity));
        modifier.spawn_persistent_modifier(
            source_actor_ref.id(),
            &context,
            &bindings,
            &mut entity_commands,
        );
    });

    // Spawn effect triggers
    for triggers in &effect.on_actor_triggers {
        let mut entity_commands = commands.entity(event.entity);
        triggers.apply(&mut entity_commands);
    }

    // Spawn effect triggers
    for triggers in &effect.on_effect_triggers {
        let mut entity_commands = commands.entity(event.targeting.target());
        triggers.apply(&mut entity_commands);
    }

    Ok(())
}

pub(crate) fn apply_effect_event_observer(
    trigger: On<ApplyEffectEvent>,
    mut actors: Query<
        (Option<&AppliedEffects>, EffectsMut),
        (Without<Effect>, Without<IsResource>),
    >,
    mut effects: Query<&Effect>,
    effect_assets: Res<Assets<EffectDef>>,
    mut writer: MessageWriter<NotifyAddStackEvent>,
    mut commands: Commands,
    type_bindings: Res<AppAttributeBindings>,
) -> Result<(), BevyError> {
    let effect = effect_assets
        .get(&trigger.handle)
        .ok_or("No effect asset.")?;

    if effect.application_policy.should_apply_now() {
        apply_instant_effect(&trigger, &mut actors, &mut commands, effect)?;
    }

    if effect.application_policy != EffectApplicationPolicy::Instant {
        spawn_persistent_effect(
            &trigger,
            &mut commands,
            effect,
            &mut actors,
            &mut effects,
            &mut writer,
            type_bindings.clone(),
        )?;
    }

    Ok(())
}
