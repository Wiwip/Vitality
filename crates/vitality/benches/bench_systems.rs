use bevy::prelude::*;
use criterion::*;
use vitality::attribute;
use vitality::prelude::*;

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

attribute!(Health);
attribute!(HealthRegen);
attribute!(MaxHealth);
attribute!(AttackPower);

fn populate_world(mut commands: Commands, entity: u32, effects: u32) {
    let mut rng = rand::rng();
    /*for _ in 0..entity {
        let player_entity = commands.spawn_empty().id();
        ActorBuilder::new(player_entity)
            .with::<Health>(12.0)
            .with::<HealthRegen>(7.0)
            .with::<Health>(100.0)
            .clamp::<Health, MaxHealth>(0.0);

        for _ in 0..effects {
            let effect = commands.spawn_empty().id();
            EffectBuilder::new(player_entity, effect)
                .with_permanent_duration()
                .with_periodic_application(rng.random_range(0.5..1.5))
                .modify_by_scalar::<Health>(rng.random_range(0.0..42.0), Additive)
                .build(&mut commands);
        }
    }*/
}

fn populate_world_by_ref(mut commands: Commands, entity: u32, effects: u32) {
    let mut rng = rand::rng();
    /*for _ in 0..entity {
        let player_entity = commands.spawn_empty().id();
        ActorBuilder::new(player_entity)
            .with::<Health>(12.0)
            .with::<HealthRegen>(7.0)
            .with::<Health>(100.0)
            .clamp::<Health, MaxHealth>(0.0);

        AttributeBuilder::<AttackPower>::new(player_entity)
            .by_ref::<Health>(0.10)
            .by_ref::<MaxHealth>(0.25)
            .commit(&mut commands);

        for _ in 0..effects {
            let effect = commands.spawn_empty().id();
            EffectBuilder::new(player_entity, effect)
                .with_permanent_duration()
                .with_periodic_application(rng.random_range(0.5..1.5))
                .modify_by_ref::<Health, HealthRegen>(1.0)
                .build(&mut commands);
        }
    }*/
}

fn criterion_benchmark(c: &mut Criterion) {
    /*c.bench_function("populate_world", |b| {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(DeathByAttributesPlugin);
        populate_world(app.world_mut().commands(), 1000, 50);
        b.iter(|| app.update())
    });

    c.bench_function("populate_world_by_ref", |b| {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(DeathByAttributesPlugin);
        populate_world_by_ref(app.world_mut().commands(), 1000, 50);
        b.iter(|| app.update())
    });*/
}
