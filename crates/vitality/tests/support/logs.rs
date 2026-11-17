#![allow(dead_code)]
//! Log capture for the cells that pin a warning. `fmt::layer()` only prints the level, so a cell
//! matching message text cannot tell `warn!` from `debug!`: these records keep `meta.level()`.

use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::subscriber::set_global_default;
use bevy::log::tracing::{Event, Level, Subscriber};
use bevy::log::tracing_subscriber::layer::{Context, SubscriberExt};
use bevy::log::tracing_subscriber::{Layer, Registry};
use std::sync::{Arc, Mutex, OnceLock};

/// one buffer per mode: the thread-local one for a cell that owns its `warn_once!` call site, the
/// process-wide one for a cell that shares its call site with another cell of the same binary
static BINARY: OnceLock<Logs> = OnceLock::new();

#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<(Level, String)>>>);

impl Logs {
    /// every record of the whole test binary: `warn_once!` is per call site, so when two cells of one
    /// binary spawn the same modifier only the first sees the record, and this is what it captured
    pub fn of_binary() -> Self {
        BINARY
            .get_or_init(|| {
                let logs = Logs::default();
                // an app that installs bevy's LogPlugin owns the global subscriber already
                let _ = set_global_default(Registry::default().with(logs.layer()));
                logs
            })
            .clone()
    }

    pub fn layer(&self) -> impl Layer<Registry> {
        Recorder(self.0.clone())
    }

    /// every record as `LEVEL message`, which is what the message assertions match
    pub fn all(&self) -> String {
        self.render(|_| true)
    }

    /// the WARN records only, which is what tells a `warn!` from a `debug!`
    pub fn warns(&self) -> String {
        self.render(|level| level == &Level::WARN)
    }

    fn render(&self, keep: impl Fn(&Level) -> bool) -> String {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(level, _)| keep(level))
            .map(|(level, message)| format!("{level:?} {message}\n"))
            .collect()
    }
}

struct Recorder(Arc<Mutex<Vec<(Level, String)>>>);

impl<S: Subscriber> Layer<S> for Recorder {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut message = Message(String::new());
        event.record(&mut message);
        self.0
            .lock()
            .unwrap()
            .push((*event.metadata().level(), message.0));
    }
}

struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}
