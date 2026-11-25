//! The fold runs in the attribute's own number type, not through f64.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;
use vitality::actors::ActorBuilder;
use vitality::effect::EffectStackingPolicy;
use vitality::effect::definition::EffectDef;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;
use vitality::store::{Assets, Handle};
use vitality::{attribute_set, init_attribute_set};
use vitality_core::value::AttributeValue;

/// mantissa in `[1, 2)` and an unbounded binary exponent, so it outgrows f64
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect)]
pub struct Endless {
    pub mantissa: f64,
    pub exponent: i64,
}

impl Endless {
    const fn new(mantissa: f64, exponent: i64) -> Self {
        Self { mantissa, exponent }
    }

    fn bits(self) -> (u64, i64) {
        (self.mantissa.to_bits(), self.exponent)
    }

    fn scale(exponent: i64) -> f64 {
        f64::from_bits(((exponent + 1023) as u64) << 52)
    }

    fn normalise(mut mantissa: f64, mut exponent: i64) -> Self {
        if mantissa == 0.0 || mantissa.is_nan() {
            return Self::new(0.0 * mantissa, 0);
        }
        // halve/double one step at a time so the mantissa stays exact
        while mantissa.abs() >= 2.0 {
            mantissa *= 0.5;
            exponent += 1;
        }
        while mantissa.abs() < 1.0 {
            mantissa *= 2.0;
            exponent -= 1;
        }
        Self::new(mantissa, exponent)
    }
}

impl std::fmt::Display for Endless {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x2^{}", self.mantissa, self.exponent)
    }
}

// a derived one would compare the mantissa before the exponent
impl PartialOrd for Endless {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.mantissa == 0.0 || other.mantissa == 0.0 {
            return self.mantissa.partial_cmp(&other.mantissa);
        }
        (self.exponent, self.mantissa).partial_cmp(&(other.exponent, other.mantissa))
    }
}

impl AttributeValue for Endless {
    fn zero() -> Self {
        Self::default()
    }

    fn one() -> Self {
        Self::new(1.0, 0)
    }

    fn add(self, rhs: Self) -> Self {
        if self.mantissa == 0.0 {
            return rhs;
        }
        if rhs.mantissa == 0.0 {
            return self;
        }
        let (wide, narrow) = if self.exponent >= rhs.exponent {
            (self, rhs)
        } else {
            (rhs, self)
        };
        let gap = wide.exponent - narrow.exponent;
        if gap > 60 {
            return wide;
        }
        Self::normalise(
            wide.mantissa + narrow.mantissa * Self::scale(-gap),
            wide.exponent,
        )
    }

    fn sub(self, rhs: Self) -> Self {
        self.add(Self::new(-rhs.mantissa, rhs.exponent))
    }

    fn mul(self, rhs: Self) -> Self {
        Self::normalise(self.mantissa * rhs.mantissa, self.exponent + rhs.exponent)
    }

    fn powi(self, exponent: u32) -> Self {
        (0..exponent).fold(Self::one(), |acc, _| acc.mul(self))
    }

    fn from_f64(value: f64) -> Self {
        if value.is_finite() {
            Self::normalise(value, 0)
        } else {
            Self::new(value, 0)
        }
    }

    fn to_f64(self) -> f64 {
        match self.exponent {
            e if e > 1023 => f64::INFINITY * self.mantissa.signum(),
            e if e < -1022 => 0.0,
            e => self.mantissa * Self::scale(e),
        }
    }
}

vitality_core::property_number!(Endless);

attribute!(Longevity, Endless);
attribute!(Weight, f64);
attribute!(Cap, Endless);

const BASE: Endless = Endless::new(1.5, 2);

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        VitalityPlugin::new()
            .attribute::<Longevity>()
            .attribute::<Weight>()
            .attribute::<Cap>(),
    ));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_micros(
        15625,
    )));
    app.update();
    app
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn spawn(app: &mut App) -> Entity {
    spawn_with(app, |hero| hero)
}

fn spawn_with(app: &mut App, extra: impl FnOnce(ActorBuilder) -> ActorBuilder) -> Entity {
    let hero = ActorBuilder::new()
        .name("Hero")
        .with::<Longevity>(BASE)
        .with::<Weight>(BASE.to_f64());
    let mut def = Some(extra(hero).build());
    let entity = app
        .world_mut()
        .run_system_once(move |mut vitality: Vitality| {
            vitality.add_spawn_actor(def.take().unwrap()).id()
        })
        .unwrap();
    ticks(app, 1);
    entity
}

fn apply(app: &mut App, target: Entity, def: EffectDef) {
    let handle: Handle<EffectDef> = app.world_mut().resource_mut::<Assets<EffectDef>>().add(def);
    app.world_mut()
        .run_system_once(move |mut effects: Effects| effects.apply(target, target, &handle))
        .unwrap();
}

fn current<T: Attribute>(app: &App, entity: Entity) -> T::Property {
    app.world().get::<T>(entity).unwrap().current_value()
}

fn base<T: Attribute>(app: &App, entity: Entity) -> T::Property {
    app.world().get::<T>(entity).unwrap().base_value()
}

#[test]
fn stacked_multipliers_pass_the_largest_f64() {
    let mut app = app();
    let hero = spawn(&mut app);
    for _ in 0..100 {
        apply(
            &mut app,
            hero,
            EffectBuilder::permanent()
                .modify::<Longevity>(Endless::new(1.0, 40), ModOp::Mul, EffectSubject::Target)
                .modify::<Weight>(2f64.powi(40), ModOp::Mul, EffectSubject::Target)
                .with_stacking_policy(EffectStackingPolicy::Add {
                    count: 1,
                    max_stack: 100,
                })
                .build(),
        );
        ticks(&mut app, 1);
    }

    // 1.5x2^2 × (2^40)^100
    assert_eq!(
        current::<Longevity>(&app, hero).bits(),
        (1.5f64.to_bits(), 4002)
    );
    assert!(current::<Weight>(&app, hero).is_infinite());
}

/// seeded LCG; no `Set`, it would end the fold
fn deal(seed: u64) -> Vec<(ModOp, Endless)> {
    const OPS: [ModOp; 4] = [ModOp::Add, ModOp::Sub, ModOp::Increase, ModOp::Mul];
    let mut state = seed;
    let mut next = move |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    (0..12)
        .map(|_| {
            let op = OPS[next(4) as usize];
            // not dyadic, so a reordered or fused step changes the bits
            let mantissa = [1.1, 1.3, 1.7, 1.9][next(4) as usize];
            (op, Endless::new(mantissa, next(6) as i64 - 3))
        })
        .collect()
}

fn fold<P: AttributeValue>(base: P, modifiers: impl Iterator<Item = (ModOp, P)>) -> P {
    let (mut add, mut sub, mut increase, mut mul) = (P::zero(), P::zero(), P::zero(), P::one());
    for (op, value) in modifiers {
        match op {
            ModOp::Add => add = P::add(add, value),
            ModOp::Sub => sub = P::add(sub, value),
            ModOp::Increase => increase = P::add(increase, value),
            ModOp::Mul => mul = P::mul(mul, value),
            ModOp::Set => unreachable!(),
        }
    }
    let sum = P::sub(P::add(base, add), sub);
    P::mul(P::mul(sum, P::add(P::one(), increase)), mul)
}

/// the sequence of `(base op value)` steps an instant effect applies
fn steps<P: AttributeValue>(base: P, modifiers: impl Iterator<Item = (ModOp, P)>) -> P {
    modifiers.fold(base, |base, modifier| fold(base, std::iter::once(modifier)))
}

fn as_f64(modifiers: &[(ModOp, Endless)]) -> impl Iterator<Item = (ModOp, f64)> + '_ {
    modifiers.iter().map(|(op, value)| (*op, value.to_f64()))
}

fn effect(policy: EffectBuilder, modifiers: &[(ModOp, Endless)]) -> EffectDef {
    modifiers
        .iter()
        .fold(policy, |builder, (op, value)| {
            builder
                .modify::<Longevity>(*value, *op, EffectSubject::Target)
                .modify::<Weight>(value.to_f64(), *op, EffectSubject::Target)
        })
        .build()
}

#[test]
fn persistent_fold_matches_the_hand_written_fold_bit_for_bit() {
    for seed in [0x2545_F491_4F6C_DD1D, 7, 1234] {
        let mut app = app();
        let hero = spawn(&mut app);
        let all = deal(seed);
        let (first, second) = all.split_at(6);
        apply(&mut app, hero, effect(EffectBuilder::permanent(), first));
        apply(&mut app, hero, effect(EffectBuilder::permanent(), second));
        ticks(&mut app, 1);

        let expected = fold(BASE, all.iter().copied());
        assert_ne!(
            expected.bits(),
            BASE.bits(),
            "the seed has to move the value"
        );
        assert_eq!(current::<Longevity>(&app, hero).bits(), expected.bits());
        assert_eq!(
            current::<Weight>(&app, hero).to_bits(),
            fold(BASE.to_f64(), as_f64(&all)).to_bits()
        );
    }
}

#[test]
fn instant_fold_matches_the_hand_written_steps_bit_for_bit() {
    for seed in [0x2545_F491_4F6C_DD1D, 7, 1234] {
        let mut app = app();
        let hero = spawn(&mut app);
        let modifiers = deal(seed);
        apply(&mut app, hero, effect(EffectBuilder::instant(), &modifiers));
        ticks(&mut app, 2);

        assert_eq!(
            base::<Longevity>(&app, hero).bits(),
            steps(BASE, modifiers.iter().copied()).bits()
        );
        assert_eq!(
            base::<Weight>(&app, hero).to_bits(),
            steps(BASE.to_f64(), as_f64(&modifiers)).to_bits()
        );
    }
}

#[test]
fn the_first_set_wins() {
    let mut app = app();
    let hero = spawn(&mut app);
    apply(
        &mut app,
        hero,
        effect(
            EffectBuilder::permanent(),
            &[(ModOp::Set, Endless::new(1.0, 3))],
        ),
    );
    apply(
        &mut app,
        hero,
        effect(
            EffectBuilder::permanent(),
            &[
                (ModOp::Set, Endless::new(1.0, 4)),
                (ModOp::Add, Endless::new(1.0, 5)),
            ],
        ),
    );
    ticks(&mut app, 1);

    assert_eq!(current::<Longevity>(&app, hero).bits(), (1f64.to_bits(), 3));
    assert_eq!(current::<Weight>(&app, hero), 8.0);
}

attribute_set! { pub struct Books { Reserve: Endless, Ledger: f64 } }

#[test]
fn attribute_set_accepts_a_host_type() {
    let mut app = app();
    app.add_plugins(init_attribute_set::<Books>);
    let carrier = app.world_mut().spawn(Books::new(Endless::one(), 2.0)).id();
    ticks(&mut app, 1);
    let boon = EffectBuilder::permanent()
        .modify::<Reserve>(Endless::new(1.0, 1024), ModOp::Mul, EffectSubject::Target)
        .modify::<Ledger>(1024.0, ModOp::Mul, EffectSubject::Target)
        .build();
    apply(&mut app, carrier, boon);
    ticks(&mut app, 1);

    assert_eq!(
        current::<Reserve>(&app, carrier).bits(),
        (1f64.to_bits(), 1024)
    );
    assert_eq!(current::<Ledger>(&app, carrier), 2048.0);
}

#[test]
fn a_clamp_reads_a_host_type_bound_past_f64() {
    let mut app = app();
    let hero = spawn_with(&mut app, |hero| {
        hero.with::<Cap>(Endless::new(1.0, 10))
            .clamp::<Longevity>(Endless::zero(), Cap::src())
    });
    let mul = |exponent| Endless::new(1.0, exponent);
    apply(
        &mut app,
        hero,
        EffectBuilder::permanent()
            .modify::<Longevity>(mul(2000), ModOp::Mul, EffectSubject::Target)
            .build(),
    );
    ticks(&mut app, 1);
    assert_eq!(
        current::<Longevity>(&app, hero).bits(),
        (1f64.to_bits(), 10)
    );

    // raising the bound past f64 re-resolves the clamped attribute in the host type
    apply(
        &mut app,
        hero,
        EffectBuilder::permanent()
            .modify::<Cap>(mul(1500), ModOp::Mul, EffectSubject::Target)
            .build(),
    );
    ticks(&mut app, 1);
    assert_eq!(
        current::<Longevity>(&app, hero).bits(),
        (1f64.to_bits(), 1510)
    );
}
