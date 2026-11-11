use crate::EffectsRef;
use crate::graph::{DependencyGraph, NodeType};
use crate::modifier::OwnedModifiers;
use crate::modifier::RecalculateExpression;
use crate::prelude::*;
use crate::{EffectSource, EffectStatusParam, EffectTarget, Stacks};
use bevy::prelude::*;
use bevy::reflect::TypeRegistryArc;
use petgraph::visit::IntoNeighbors;
use std::any::type_name;
use std::collections::HashSet;
use std::marker::PhantomData;
use vitality_core::attributes::AttributeDependents;
use vitality_core::attributes::{
    Attribute, AttributeQueryData, AttributeQueryDataReadOnly, ManageAttributes,
};
use vitality_core::modifier::AttributeCalculator;
use vitality_core::{AppAttributeBindings, CurrentValueChanged};

#[derive(EntityEvent)]
#[entity_event(propagate=&'static EffectTarget, auto_propagate)]
pub struct MarkNodeDirty<T: Attribute> {
    pub entity: Entity,
    pub phantom_data: PhantomData<T>,
}

/// Observes [`MarkNodeDirty`] triggers for specific attributes and propagates
/// the event upward through using the [`EffectTarget`] chain.
///
/// Stops when it encounters a dirty node indicating that all later nodes are already dirty.
pub fn mark_node_dirty_observer<T: Attribute>(
    mut trigger: On<MarkNodeDirty<T>>,
    dirty_nodes: Query<&Dirty<T>>,
    mut commands: Commands,
) {
    if dirty_nodes.contains(trigger.entity) {
        trigger.propagate(false);
        return;
    }
    commands
        .entity(trigger.entity)
        .try_insert(Dirty::<T>::default());
}

/// The actors of one attribute that carry a dirty mark.
type DirtyActors<'w, 's, T> =
    Query<'w, 's, Entity, (With<ManageAttributes>, With<T>, With<Dirty<T>>)>;

/// Navigates the tree descendants to update the tree attribute values
/// Effects that have a periodic timer application must be ignored in the current value calculations
pub fn update_current_value_system<T: Attribute>(
    graph: DependencyGraph,
    nodes: Query<&NodeType>,
    dirty_actors: DirtyActors<'_, '_, T>,
    dirty_nodes: Query<&Dirty<T>>,
    statuses: Query<EffectStatusParam>,
    attributes: Query<AttributeQueryDataReadOnly<T>>,
    attribute_refs: Query<EffectsRef>,
    effects: Query<(
        &OwnedModifiers,
        &EffectSource,
        &EffectTarget,
        Option<&Stacks>,
    )>,
    modifiers: Query<&AttributeModifier<T>>,
    mut commands: Commands,
    type_registry: Res<AppTypeRegistry>,
    type_bindings: Res<AppAttributeBindings>,
) {
    debug_once!("Ready: update_effect_tree_system::{}", type_name::<T>());
    for actor_entity in dirty_actors.iter() {
        let calculator = update_effect_tree_attributes::<T>(
            actor_entity,
            &graph,
            &nodes,
            &dirty_nodes,
            &statuses,
            &attributes,
            &attribute_refs,
            &effects,
            &modifiers,
            &mut commands,
            &type_registry.0.clone(),
            &type_bindings,
        );

        // Signal to update the attribute
        commands.trigger(UpdateAttributeSignal {
            entity: actor_entity,
            calculator,
        });

        // Cleans the node
        commands.entity(actor_entity).try_remove::<Dirty<T>>();
    }
}

fn update_effect_tree_attributes<T: Attribute>(
    current_entity: Entity,
    graph: &DependencyGraph,
    nodes: &Query<&NodeType>,
    dirty_nodes: &Query<&Dirty<T>>,
    statuses: &Query<EffectStatusParam>,
    attributes: &Query<AttributeQueryDataReadOnly<T>>,
    attribute_refs: &Query<EffectsRef>,
    effects: &Query<(
        &OwnedModifiers,
        &EffectSource,
        &EffectTarget,
        Option<&Stacks>,
    )>,
    modifiers: &Query<&AttributeModifier<T>>,
    commands: &mut Commands,
    type_registry: &TypeRegistryArc,
    type_bindings: &AppAttributeBindings,
) -> AttributeCalculator<T> {
    let Ok(node_type) = nodes.get(current_entity) else {
        error!("{}: Error getting node type.", current_entity);
        return AttributeCalculator::default();
    };
    let Ok(status) = statuses.get(current_entity) else {
        debug!("{}: Error getting status.", current_entity);
        return AttributeCalculator::default();
    };
    if status.is_periodic() || status.is_inactive() {
        return AttributeCalculator::default();
    }
    if !dirty_nodes.contains(current_entity) {
        // not dirty and no cached calculator: keep walking the tree
        if let Ok(attribute) = attributes.get(current_entity) {
            return attribute.calculator_cache.calculator;
        }
    }

    let node_calculator = match node_type {
        NodeType::Actor => {
            // Traverse children
            graph
                .neighbors(current_entity)
                .map(|entity| {
                    update_effect_tree_attributes::<T>(
                        entity,
                        graph,
                        nodes,
                        dirty_nodes,
                        statuses,
                        attributes,
                        attribute_refs,
                        effects,
                        modifiers,
                        commands,
                        type_registry,
                        type_bindings,
                    )
                })
                .fold(AttributeCalculator::default(), |acc, child| {
                    acc.combine(child)
                })
        }
        NodeType::Effect => {
            let Ok((modifier_entities, _source, _target, stacks)) = effects.get(current_entity)
            else {
                // Effect has no modifiers, return default calculator.
                return AttributeCalculator::default();
            };

            modifier_entities
                .iter()
                .filter_map(|modifier_entity| {
                    // Get the modifier
                    let Ok(modifier) = modifiers.get(modifier_entity) else {
                        return None;
                    };
                    // Get the effect to check if we should apply it
                    let Ok(status) = statuses.get(current_entity) else {
                        return None;
                    };
                    if status.is_periodic() || status.is_inactive() {
                        return None;
                    }

                    let stacks = stacks.map_or(1, |stacks| stacks.current_value());
                    let calc = AttributeCalculator::from_op_stacked(
                        modifier.operation,
                        modifier.value,
                        stacks,
                    );

                    Some(calc)
                })
                .fold(AttributeCalculator::default(), |acc, child| {
                    acc.combine(child)
                })
        }
    };

    // Signal to update the attribute
    commands.trigger(UpdateAttributeSignal::<T> {
        entity: current_entity,
        calculator: node_calculator,
    });

    // Cleans the node
    commands.entity(current_entity).try_remove::<Dirty<T>>();

    node_calculator
}

#[derive(EntityEvent)]
pub struct UpdateAttributeSignal<T: Attribute> {
    entity: Entity,
    calculator: AttributeCalculator<T>,
}

pub fn update_attribute<T: Attribute>(
    trigger: On<UpdateAttributeSignal<T>>,
    mut attributes: Query<(AttributeQueryData<T>, Option<&AttributeDependents<T>>)>,
    mut commands: Commands,
) {
    if let Ok((mut attribute, dependencies)) = attributes.get_mut(trigger.event_target()) {
        attribute.calculator_cache.calculator = trigger.event().calculator;
        let old_value = attribute.attribute.current_value();

        let should_notify_observers = attribute.update_attribute(&trigger.event().calculator);
        if should_notify_observers {
            commands.trigger(CurrentValueChanged::<T> {
                entity: trigger.event_target(),
                phantom_data: Default::default(),
                old: old_value,
                new: attribute.attribute.current_value(),
            });
            // Notify dependencies
            if let Some(dependencies) = dependencies {
                dependencies.iter().for_each(|dep| {
                    commands.trigger(RecalculateExpression {
                        modifier_entity: dep,
                    })
                })
            }
        }
    };
}

#[derive(Component, Copy, Clone, Debug)]
#[component(storage = "SparseSet")]
pub struct Dirty<T> {
    phantom: PhantomData<T>,
}

impl<T> Default for Dirty<T> {
    fn default() -> Self {
        Self {
            phantom: PhantomData,
        }
    }
}

pub fn on_add_attribute<T: Attribute>(trigger: On<Insert<T>>, mut commands: Commands) {
    commands.trigger(MarkNodeDirty::<T> {
        entity: trigger.event_target(),
        phantom_data: Default::default(),
    });
}

#[derive(EntityEvent)]
pub struct AttributeDependencyChanged<T> {
    pub entity: Entity,
    phantom_data: PhantomData<T>,
}

/// "Something about this actor's effects changed and no attribute of it did", so an actor can be dirtied
/// from code that is not typed on an attribute. [`init_effect_attribute`] registers one observer per
/// attribute, and each answers with its own [`MarkNodeDirty`].
#[derive(EntityEvent)]
pub(crate) struct MarkActorDirty {
    pub entity: Entity,
}

pub(crate) fn mark_actor_dirty_observer<T: Attribute>(
    trigger: On<MarkActorDirty>,
    mut commands: Commands,
) {
    commands.trigger(MarkNodeDirty::<T> {
        entity: trigger.entity,
        phantom_data: Default::default(),
    });
}

pub fn on_change_notify_attribute_dependencies<T: Attribute>(
    query: Query<&AttributeDependents<T>, Changed<T>>,
    mut commands: Commands,
) {
    for dependents in query.iter() {
        let unique_entities: HashSet<Entity> = dependents.iter().collect();
        let notify_targets: Vec<Entity> = unique_entities.into_iter().collect();

        notify_targets.iter().for_each(|target| {
            commands.trigger(AttributeDependencyChanged::<T> {
                entity: *target,
                phantom_data: Default::default(),
            });
        });
    }
}

pub fn on_change_notify_attribute_parents<T: Attribute>(
    query: Query<Entity, Changed<T>>,
    mut commands: Commands,
) {
    for entity in query.iter() {
        commands.trigger(MarkNodeDirty::<T> {
            entity,
            phantom_data: Default::default(),
        });
    }
}
