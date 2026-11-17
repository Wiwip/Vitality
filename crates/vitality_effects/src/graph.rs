//! a modifier applies to the attribute of the actor named by its [`EffectSubject`](crate::modifier::EffectSubject) — the target, or the source for a `Source` modifier

use bevy::prelude::*;

#[derive(Component, Default, Copy, Clone, Reflect, Debug)]
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
