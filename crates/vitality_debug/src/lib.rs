pub mod debug_overlay;

use crate::debug_overlay::{explore_actors_system, setup_debug_overlay};
use bevy_app::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;
use std::time::Duration;
use vitality_core::bevy_time::common_conditions::on_timer;

// the tree walks every actor, effect and modifier, so a frame-rate redraw is wasted work
const REFRESH_INTERVAL: Duration = Duration::from_millis(250);

pub struct ActorInspectorPlugin;

impl Plugin for ActorInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_debug_overlay);
        app.add_systems(
            Update,
            explore_actors_system.run_if(on_timer(REFRESH_INTERVAL)),
        );
    }
}
