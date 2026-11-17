//! The missing-attribute fallback: a `T::src()` read of an attribute the actor lacks reads zero.
//! `warn_once!` is per call site, so one warning covers both actors and every tick of the run.

mod gameplay;

use bevy::log::tracing::subscriber::with_default;
use bevy::log::tracing_subscriber::{Registry, fmt, layer::SubscriberExt};
use gameplay::*;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use vitality::actors::ActorBuilder;
use vitality::effect::modifier::{EffectSubject, ModOp};
use vitality::prelude::*;

/// The resolver settles a dependent hop in the same tick, so 4 leaves room.
const SETTLE: usize = 4;

fn drain() -> EffectBuilder {
    EffectBuilder::permanent().modify::<Health>(Power::src(), ModOp::Sub, EffectSubject::Target)
}

#[derive(Clone)]
struct Sink(Arc<Mutex<String>>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap()
            .push_str(&String::from_utf8_lossy(buf));
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn reading_an_attribute_the_actor_lacks_is_zero() {
    let logs = Arc::new(Mutex::new(String::new()));
    let writer = logs.clone();
    let subscriber = Registry::default().with(
        fmt::layer()
            .with_ansi(false)
            .without_time()
            .with_writer(move || Sink(writer.clone())),
    );

    // thread-local layer: the global one LogPlugin::build installs would race the other tests here
    with_default(subscriber, || {
        let mut app = app();
        let full = spawn(
            &mut app,
            ActorBuilder::new()
                .name("full")
                .with::<Health>(100.0)
                .with::<Power>(10.0),
        );
        let bare = spawn(
            &mut app,
            ActorBuilder::new().name("bare").with::<Health>(100.0),
        );
        assert!(
            !has::<Power>(&app, bare),
            "the bare actor has to lack the attribute the shared definition reads"
        );

        let drain = effect(&mut app, drain().build());
        apply(&mut app, full, full, &drain);
        apply(&mut app, bare, bare, &drain);
        ticks(&mut app, SETTLE);

        // equality is the NaN guard too: NaN compares false against every number
        assert_eq!(current::<Health>(&app, full), 90.0);
        assert_eq!(
            current::<Health>(&app, bare),
            100.0,
            "the missing Power has to read as zero, not NaN"
        );

        ticks(&mut app, SETTLE);
        assert_eq!(current::<Health>(&app, full), 90.0);
        assert_eq!(base::<Health>(&app, bare), 100.0);
        assert_eq!(
            current::<Health>(&app, bare),
            100.0,
            "the fallback has to be stable across ticks, not creep"
        );
    });

    let logs = logs.lock().unwrap();
    let warned = logs.matches("reading it as zero").count();
    assert_eq!(
        warned,
        1,
        "one missing-attribute read across two actors and {} ticks has to warn once, saw {warned}: {logs:?}",
        2 * SETTLE
    );
}
