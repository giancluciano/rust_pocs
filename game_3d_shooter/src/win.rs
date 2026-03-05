use bevy::prelude::*;

use crate::AppState;

pub struct WinPlugin;

#[derive(Component)]
struct WinUi;

impl Plugin for WinPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::YouWin), spawn_win_screen)
            .add_systems(OnExit(AppState::YouWin), cleanup_win)
            .add_systems(
                Update,
                win_button.run_if(in_state(AppState::YouWin)),
            );
    }
}

fn spawn_win_screen(mut commands: Commands) {
    commands.spawn((Camera2d, WinUi));

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
            BackgroundColor(Color::srgb(0.0, 0.1, 0.05)),
            WinUi,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("YOU WIN!"),
                TextFont {
                    font_size: 80.0,
                    ..default()
                },
                TextColor(Color::srgb(0.2, 1.0, 0.3)),
            ));

            parent.spawn((
                Text::new("You shot the target!"),
                TextFont {
                    font_size: 28.0,
                    ..default()
                },
                TextColor(Color::srgb(0.8, 0.8, 0.8)),
            ));

            parent
                .spawn((
                    Button,
                    Node {
                        width: Val::Px(220.0),
                        height: Val::Px(55.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.15, 0.2, 0.55)),
                ))
                .with_child((
                    Text::new("MAIN MENU"),
                    TextFont {
                        font_size: 28.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
        });
}

fn cleanup_win(mut commands: Commands, query: Query<Entity, With<WinUi>>) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
}

fn win_button(
    interaction_query: Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for interaction in &interaction_query {
        if *interaction == Interaction::Pressed {
            next_state.set(AppState::MainMenu);
        }
    }
}
