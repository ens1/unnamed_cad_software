//! Desktop redraw policy and the adapter for the pinned Bevy/egui integration.
//!
//! Bevy 0.20 defaults to a game loop. bevy_egui 0.43.1 also turns *delayed*
//! repaint requests into immediate RequestRedraw messages. Keep its complete
//! output handling, but schedule its redraw independently. Revisit this adapter
//! when either dependency changes its output or event-loop integration.
use bevy::{
    ecs::schedule::ScheduleCleanupPolicy,
    prelude::*,
    window::RequestRedraw,
    winit::{EventLoopProxy, EventLoopProxyWrapper, UpdateMode, WinitSettings, WinitUserEvent},
};
use bevy_egui::{EguiFullOutput, EguiPostUpdateSet};
use std::{
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};

const MAINTENANCE_INTERVAL: Duration = Duration::from_millis(400);

struct WakeState {
    proxy: EventLoopProxy<WinitUserEvent>,
    deadline: Arc<(Mutex<Option<Instant>>, Condvar)>,
}

static WAKE: OnceLock<WakeState> = OnceLock::new();

pub struct RedrawPlugin;

impl Plugin for RedrawPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WinitSettings {
            focused_mode: UpdateMode::reactive_low_power(MAINTENANCE_INTERVAL),
            unfocused_mode: UpdateMode::reactive_low_power(MAINTENANCE_INTERVAL),
        });

        app.add_systems(Startup, initialize_wake)
            .add_systems(PostStartup, install_output_adapter);
    }
}

fn install_output_adapter(world: &mut World) {
    // Removal initializes the affected schedule. Wait until render plugins and
    // Startup resources are ready, while still preceding the first PostUpdate.
    // Keep all other egui systems and dependencies; replace only its output
    // system. RedrawPlugin must be installed after EguiPlugin.
    world.schedule_scope(PostUpdate, |world, schedule| {
        let removed = schedule
            .remove_systems_in_set(
                bevy_egui::output::process_output_system,
                world,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            )
            .expect("The pinned egui output system must be available to adapt redraw scheduling");
        assert_eq!(removed, 1, "Expected exactly one egui output system");
        schedule.add_systems(
            process_output
                .in_set(EguiPostUpdateSet::ProcessOutput)
                .before(bevy_egui::input::process_ime_system),
        );
    });
}

fn initialize_wake(proxy: Res<EventLoopProxyWrapper>) {
    let proxy = (**proxy).clone();
    WAKE.get_or_init(|| {
        let deadline = Arc::new((Mutex::new(None::<Instant>), Condvar::new()));
        let timer_deadline = Arc::clone(&deadline);
        let timer_proxy = proxy.clone();
        std::thread::Builder::new()
            .name("tcad-redraw-deadline".into())
            .spawn(move || {
                let (deadline, changed) = &*timer_deadline;
                let mut pending = deadline.lock().unwrap_or_else(|error| error.into_inner());
                loop {
                    let Some(at) = *pending else {
                        pending = changed
                            .wait(pending)
                            .unwrap_or_else(|error| error.into_inner());
                        continue;
                    };
                    let wait = at.saturating_duration_since(Instant::now());
                    if wait.is_zero() {
                        *pending = None;
                        drop(pending);
                        if timer_proxy.send_event(WinitUserEvent::WakeUp).is_err() {
                            break;
                        }
                        pending = deadline.lock().unwrap_or_else(|error| error.into_inner());
                    } else {
                        (pending, _) = changed
                            .wait_timeout(pending, wait)
                            .unwrap_or_else(|error| error.into_inner());
                    }
                }
            })
            .expect("Cannot start the desktop repaint deadline scheduler");
        WakeState { proxy, deadline }
    });
}

/// Notify the desktop after publishing a background result. Startup races are
/// harmless: the bounded maintenance update also drains completed work.
pub fn wake() {
    if let Some(state) = WAKE.get() {
        let _ = state.proxy.send_event(WinitUserEvent::WakeUp);
    }
}

/// Request a future update without continuously rendering while waiting. All
/// callers share one pending deadline and one sleeping thread; an earlier
/// request supersedes a later one. An obsolete deadline causes at most one
/// harmless extra update.
pub fn request_after(delay: Duration) {
    if delay.is_zero() {
        wake();
        return;
    }
    if delay >= MAINTENANCE_INTERVAL {
        // The maintenance heartbeat already guarantees an update this soon.
        return;
    }
    let Some(state) = WAKE.get() else {
        return;
    };
    let Some(at) = Instant::now().checked_add(delay) else {
        return;
    };
    let (deadline, changed) = &*state.deadline;
    let mut pending = deadline.lock().unwrap_or_else(|error| error.into_inner());
    if pending.is_none_or(|previous| at < previous) {
        *pending = Some(at);
        changed.notify_one();
    }
}

fn process_output(world: &mut World) {
    let delay = world
        .query::<&EguiFullOutput>()
        .iter(world)
        .filter_map(|output| output.0.as_ref())
        .flat_map(|output| output.viewport_output.values())
        .map(|viewport| viewport.repaint_delay)
        .min()
        .unwrap_or(Duration::MAX);

    // This exclusive invocation isolates only the original output system's
    // redraw messages. Other producers' message history, IDs and reader cursors
    // remain intact. No scheduled system runs while the scratch buffer exists.
    let requested = world.resource_scope(|world, _: Mut<Messages<RequestRedraw>>| {
        world.insert_resource(Messages::<RequestRedraw>::default());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            world.run_system_cached(bevy_egui::output::process_output_system)
        }));
        // Remove the scratch resource before resource_scope restores the real
        // message buffer, including when system validation reports an error.
        let scratch = world
            .remove_resource::<Messages<RequestRedraw>>()
            .expect("The isolated redraw buffer must remain available");
        let result = match result {
            Ok(result) => result,
            // Preserve the original panic while allowing resource_scope to
            // restore the real message buffer without a second unwind panic.
            Err(payload) => std::panic::resume_unwind(payload),
        };
        result.expect("The pinned egui output system must run in its original schedule phase");
        !scratch.is_empty()
    });

    if requested {
        if delay.is_zero() {
            world
                .resource_mut::<Messages<RequestRedraw>>()
                .write(RequestRedraw);
        } else if delay != Duration::MAX {
            request_after(delay);
        }
    }
}
