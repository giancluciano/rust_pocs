use bevy::prelude::*;

use crate::AppState;

pub struct MenuPlugin;

#[derive(Component)]
struct MenuUi;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::MainMenu), spawn_menu)
            .add_systems(OnExit(AppState::MainMenu), cleanup_menu)
            .add_systems(
                Update,
                menu_button.run_if(in_state(AppState::MainMenu)),
            );
    }
}

fn spawn_menu(mut commands: Commands) {
    commands.spawn((Camera2d, MenuUi));

    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(30.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.05, 0.05, 0.1)),
            MenuUi,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("3D SHOOTER"),
                TextFont {
                    font_size: 72.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));

            parent
                .spawn((
                    Button,
                    Node {
                        width: Val::Px(180.0),
                        height: Val::Px(55.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.15, 0.55, 0.15)),
                ))
                .with_child((
                    Text::new("PLAY"),
                    TextFont {
                        font_size: 32.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
        });
}

fn cleanup_menu(mut commands: Commands, query: Query<Entity, With<MenuUi>>) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
}

fn menu_button(
    interaction_query: Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for interaction in &interaction_query {
        if *interaction == Interaction::Pressed {
            next_state.set(AppState::InGame);
        }
    }
}
