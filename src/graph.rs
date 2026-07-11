use crate::effect::{AppliedEffects, EffectSource};
use bevy::ecs::VariantDefaults;
use bevy::ecs::system::SystemParam;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use petgraph::visit::{DfsEvent, Visitable, depth_first_search};
use petgraph::visit::{GraphBase, IntoNeighbors};
use ptree::{TreeBuilder, print_tree};
use std::collections::HashSet;

/// Attributes are Components and inserted on Entities.
/// - Derived attributes could be used
/// Effects are spawned as child of entities
/// - Effects can modify the modifiers till now, such as an intensity metric or stacks
/// Modifiers are added to Effects as Vec<Mod>
/// - Modifiers must apply to an attribute

#[derive(Component, Default, VariantDefaults, Copy, Clone, Reflect, Debug)]
pub enum NodeType {
    #[default]
    Actor,
    Effect,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

// Lightweight wrapper that implements petgraph traits
#[derive(SystemParam)]
pub struct DependencyGraph<'w, 's> {
    node_type: Query<'w, 's, Read<NodeType>>,
    applied_effects: Query<'w, 's, (Entity, Read<AppliedEffects>)>,
    effect_sources: Query<'w, 's, (Entity, Read<EffectSource>)>,
}

impl DependencyGraph<'_, '_> {
    fn node_type(&self, entity: Entity) -> Option<&NodeType> {
        self.node_type.get(entity).ok()
    }

    pub fn print_dependencies(&self, entity: Entity) {
        let node_type = self.node_type(entity).expect("Node type not found");
        let mut tree = TreeBuilder::new("Root".into());
        tree.begin_child(format!("{}({})", node_type, entity.to_string()));

        // Use petgraph's depth_first_search with a custom visitor
        depth_first_search(&self, Some(entity), |event| {
            match event {
                DfsEvent::Discover(_entity, _time) => {}
                DfsEvent::TreeEdge(_source, target) => {
                    let node_type = self.node_type(target).expect("Node type not found");
                    tree.begin_child(format!("{}({})", node_type, target));
                }
                DfsEvent::BackEdge(_source, target) => {
                    let node_type = self.node_type(target).expect("Node type not found");
                    tree.begin_child(format!("{}({})", node_type, target));
                    tree.end_child();
                }
                DfsEvent::CrossForwardEdge(source, target) => {
                    warn!("Cross edge: {} -> {}", source, target);
                }
                DfsEvent::Finish(_entity, _time) => {
                    tree.end_child();
                }
            }
            petgraph::visit::Control::<Entity>::Continue
        });

        let final_tree = tree.build();
        print_tree(&final_tree).expect("Failed to print tree");
    }
}

impl GraphBase for DependencyGraph<'_, '_> {
    type NodeId = Entity;
    type EdgeId = (Entity, Entity);
}

impl IntoNeighbors for &DependencyGraph<'_, '_> {
    type Neighbors = vec::IntoIter<Entity>;

    fn neighbors(self, node: Self::NodeId) -> Self::Neighbors {
        let node_type = self.node_type.get(node).expect("Error getting node type.");

        let neighbours = match *node_type {
            NodeType::Actor => self
                .applied_effects
                .get(node)
                .map(|(_, effects)| effects.iter().collect::<Vec<_>>())
                .unwrap_or_default(),
            NodeType::Effect => self
                .effect_sources
                .get(node)
                .map(|(_, source)| vec![source.0])
                .unwrap_or_default(),
        };

        neighbours.into_iter()
    }
}

impl Visitable for DependencyGraph<'_, '_> {
    type Map = HashSet<Entity>;
    fn visit_map(&self) -> Self::Map {
        HashSet::new()
    }
    fn reset_map(&self, map: &mut Self::Map) {
        map.clear();
    }
}

/*
#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::ActorBuilder;
    use crate::assets::ActorDef;
    use crate::context::EffectContext;
    use crate::{AttributesPlugin, ReflectAccessAttribute};
    use crate::{attribute, init_attribute};

    attribute!(Health, f64);
    attribute!(Mana, f64);
    attribute!(Strength, f64);

    #[derive(Resource)]
    struct Definitions {
        actor_def: Handle<ActorDef>,
    }

    #[test]
    fn test_simple_modifier() {
        let mut app = App::new();
        init_app(&mut app);

        app.add_systems(
            Startup,
            (
                create_test_actor,
                |mut ctx: EffectContext, def: Res<Definitions>| {
                    let test_actor = ctx.spawn_actor(&def.actor_def).id();

                    let effect = EffectBuilder::permanent()
                        .modify::<Health>(Mod::Add(10.0), Who::Source)
                        .modify::<Mana>(Mod::More(1.0), Who::Source)
                        .build();

                    ctx.apply_dynamic_effect_to_self(test_actor, effect);
                },
            )
                .chain(),
        );

        app.add_systems(Update, |actor: Query<(&Health, &Mana)>| {
            let (health, mana) = actor.single().unwrap();
            assert!((health.current_value - 110.0).abs() < f64::EPSILON);
            assert!((mana.current_value - 2000.0).abs() < f64::EPSILON);
        });

        app.update();
    }

    fn init_app(app: &mut App) {
        app.add_plugins(MinimalPlugins)
            .add_plugins(AssetPlugin::default())
            .add_plugins(AttributesPlugin)
            .add_plugins((
                init_attribute::<Health>,
                init_attribute::<Mana>,
                init_attribute::<Strength>,
            ));
    }

    fn create_test_actor(mut actor_assets: ResMut<Assets<ActorDef>>, mut commands: Commands) {
        let actor_template = actor_assets.add(
            ActorBuilder::new()
                .with::<Strength>(12.0)
                .with::<Health>(100.0)
                .with::<Mana>(1000.0)
                .build(),
        );
        commands.insert_resource(Definitions {
            actor_def: actor_template,
        });
    }
}
*/
