mod game;
mod menu;
mod win;

use avian3d::prelude::*;
use bevy::prelude::*;

#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    MainMenu,
    InGame,
    YouWin,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "3D Shooter".into(),
                ..default()
            }),
            ..default()
        }))
        .init_state::<AppState>()
        .add_plugins((
            PhysicsPlugins::default(),
            menu::MenuPlugin,
            game::GamePlugin,
            win::WinPlugin,
        ))
        .run();
}
