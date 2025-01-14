pub mod resources;

pub mod components;
pub mod events;
pub mod icon;
pub mod systems;

use crate::core::window::components::{PrimaryWindow, Window};
use crate::core::window::events::CloseRequestedEvent;
use crate::core::window::resources::{PrimaryWindowCount, WinitWindows};
use crate::core::window::systems::{
    pu_exit_on_all_closed, pu_exit_on_primary_closed, u_close_windows, u_despawn_windows,
    u_primary_window_check,
};
use bevy_app::prelude::*;
use bevy_app::{AppExit, PluginsState};
use bevy_ecs::event::ManualEventReader;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;
use log::{error, info};
use winit::event::{Event, StartCause, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};

/// The plugin which adds a window and associated systems to the app.
///
/// It also overrides the default bevy runner with an event loop.
/// See following fields for how to exit the loop.
pub struct WindowPlugin {
    /// The primary window to create at the start of the program
    /// Can be `None` if no primary window is desired
    /// Set primary window parameters here
    pub primary_window: Option<Window>,
    /// The condition at which the event loop will exit.
    ///
    /// See `ExitCondition` for more information.
    pub exit_condition: ExitCondition,
}

impl Default for WindowPlugin {
    fn default() -> Self {
        WindowPlugin {
            primary_window: Some(Window::default()),
            exit_condition: ExitCondition::default(),
        }
    }
}

impl Plugin for WindowPlugin {
    fn build(&self, app: &mut App) {
        // Register events
        app.add_event::<CloseRequestedEvent>();

        // If a primary window is specified, spawn the entity with the window
        if let Some(primary_window) = &self.primary_window {
            app.world
                .spawn(primary_window.clone())
                .insert(PrimaryWindow);
        }

        // Add systems to exit the event loop when the condition is met
        match self.exit_condition {
            ExitCondition::OnPrimaryClosed => {
                app.add_systems(PostUpdate, pu_exit_on_primary_closed);
            }
            ExitCondition::OnAllClosed => {
                app.add_systems(PostUpdate, pu_exit_on_all_closed);
            }
            ExitCondition::DontExit => {}
        }

        // Insert resources
        app.insert_non_send_resource(EventLoop::new().unwrap()); // Event loop created here
        app.insert_non_send_resource(WinitWindows::default());
        app.insert_resource(PrimaryWindowCount::default());

        // Add systems
        app.add_systems(Update, u_primary_window_check);
        app.add_systems(Update, u_close_windows);
        app.add_systems(Update, u_despawn_windows);

        // Set event loop runner
        app.set_runner(runner);
    }
}

fn runner(mut app: App) {
    // Bevy stuff that I don't understand
    // Apparently if plugin loading is ready, we need to call finish and cleanup
    if app.plugins_state() == PluginsState::Ready {
        app.finish();
        app.cleanup();
    }

    // Get the event loop from resources
    let event_loop = app
        .world
        .remove_non_send_resource::<EventLoop<()>>()
        .unwrap();

    // System state of added window component
    // We will use this in the event loop to create any new windows that were added
    let mut create_windows_system_state: SystemState<(
        Query<(Entity, &Window), Added<Window>>,
        NonSendMut<WinitWindows>,
    )> = SystemState::from_world(&mut app.world);

    // Event reader to read any app exit events
    let mut app_exit_event_reader = ManualEventReader::<AppExit>::default();

    // ! Temporary fix of extra AboutToWait events on windows
    let mut exited = false;

    let event_handler = move |event: Event<()>, window_target: &EventLoopWindowTarget<()>| {
        // Close the event loop if there is any app exit events
        if let Some(app_exit_events) = app.world.get_resource::<Events<AppExit>>() {
            if app_exit_event_reader.read(app_exit_events).last().is_some() {
                window_target.exit();
                exited = true;
                return;
            }
        }

        match event {
            // Start of the event loop
            Event::NewEvents(StartCause::Init) => {
                // Create any new windows
                let (query, winit_windows) = create_windows_system_state.get_mut(&mut app.world);
                create_windows(query, winit_windows, window_target);
                create_windows_system_state.apply(&mut app.world);
            }
            // Send a close requested event so systems can drop the Window and despawn windows
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
            } => {
                // Close window
                app.world.send_event(CloseRequestedEvent { window_id });
            }
            // This is where the frame happens
            Event::AboutToWait => {
                // Don't update if plugins are not ready
                if app.plugins_state() == PluginsState::Cleaned && !exited {
                    // Run the frame
                    app.update();

                    // Close event loop if received events
                    if let Some(app_exit_events) = app.world.get_resource::<Events<AppExit>>() {
                        if app_exit_event_reader.read(app_exit_events).last().is_some() {
                            window_target.exit();
                            exited = true;
                            return;
                        }
                    }

                    // TODO: shouldn't we do the same plugin functions as at the top?
                }
            }
            _ => {}
        };

        // Create any new windows that were added
        let (query, winit_windows) = create_windows_system_state.get_mut(&mut app.world);
        create_windows(query, winit_windows, window_target);
        create_windows_system_state.apply(&mut app.world);
    };

    // This ensures that new events will be started whenever possible
    event_loop.set_control_flow(ControlFlow::Poll);

    // Run event loop
    info!("Entered event loop");
    if let Err(err) = event_loop.run(event_handler) {
        error!("winit event loop error: {err}");
    }
}

/// Function called to create any winit windows after a new Window component is spawned
fn create_windows(
    query: Query<(Entity, &Window), Added<Window>>,
    mut winit_windows: NonSendMut<WinitWindows>,
    event_loop: &EventLoopWindowTarget<()>,
) {
    for (entity, window) in query.iter() {
        // If the winit window already exists somehow, don't create another one
        if winit_windows.entity_to_window.contains_key(&entity) {
            continue;
        }

        winit_windows.create_window(event_loop, entity, window);
    }
}

/// The condition at which the event loop will quit
#[allow(dead_code)]
#[derive(Default)]
pub enum ExitCondition {
    /// Quit when the primary window is closed
    OnPrimaryClosed,
    /// Quit when all windows are closed
    #[default]
    OnAllClosed,
    /// Don't quit no matter what
    DontExit,
}
