//! Effects of gadgets on people, and short-lived light flashes:
//!
//! - [`Flashed`] (simulation state on a character): seconds of blindness
//!   and deafness left after a flashbang. The local player's screen goes
//!   white (fading out over the last part), and [`HearingMuffled`] tells
//!   audio code to muffle; as a bridge until every sound reads it, the
//!   global volume is lowered too.
//! - [`LightFlash`]: a point light that fades out quickly (blasts).
//! - The view shakes near a working ring cutter.

#[cfg(feature = "client")]
use bevy::audio::{GlobalVolume, Volume};
use bevy::prelude::*;

use super::{
    GadgetStage,
    cutter::{CutterState, RingCutter},
    remote_view::RemoteViewCamera,
};
use crate::{config::Tuning, player::LocalPlayer};

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HearingMuffled>()
            .add_systems(Startup, spawn_flash_overlay)
            .add_systems(FixedUpdate, wear_off.in_set(GadgetStage::Effects))
            .add_systems(Update, (show_flash, muffle_hearing, fade_lights))
            .add_systems(
                PostUpdate,
                shake_near_cutters.before(TransformSystems::Propagate),
            );
        // No audio in the dedicated-server build.
        #[cfg(feature = "client")]
        app.add_systems(Update, lower_volume.after(muffle_hearing));
    }
}

/// Blinded / deafened by a flashbang. Seconds left, and the starting
/// amounts (for fading).
/// Replicated to the affected player only (`net/protocol.rs`).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Flashed {
    pub blind: f32,
    pub blind_max: f32,
    pub deaf: f32,
    pub deaf_max: f32,
}

impl Flashed {
    /// A second flash: keep whichever is worse.
    pub fn merge(&mut self, other: Flashed) {
        if other.blind > self.blind {
            self.blind = other.blind;
            self.blind_max = other.blind_max;
        }
        if other.deaf > self.deaf {
            self.deaf = other.deaf;
            self.deaf_max = other.deaf_max;
        }
    }

    /// 0..1 how white the screen is: full, then fading over the last 40%.
    pub fn whiteness(&self) -> f32 {
        if self.blind <= 0.0 || self.blind_max <= 0.0 {
            return 0.0;
        }
        let fade = self.blind_max * 0.4;
        (self.blind / fade.max(0.01)).min(1.0)
    }
}

/// How muffled the local player's hearing is, 0..1. Audio code should
/// scale volumes by `1 - 0.85 * amount` (gadget sounds do).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct HearingMuffled {
    pub amount: f32,
}

fn wear_off(time: Res<Time>, mut flashed: Query<(Entity, &mut Flashed)>, mut commands: Commands) {
    let dt = time.delta_secs();
    for (entity, mut f) in &mut flashed {
        f.blind = (f.blind - dt).max(0.0);
        f.deaf = (f.deaf - dt).max(0.0);
        if f.blind <= 0.0 && f.deaf <= 0.0 {
            commands.entity(entity).remove::<Flashed>();
        }
    }
}

#[derive(Component)]
struct FlashOverlay;

fn spawn_flash_overlay(mut commands: Commands) {
    commands.spawn((
        Name::new("Flash overlay"),
        FlashOverlay,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 0.97, 0.0)),
        GlobalZIndex(100),
        Pickable::IGNORE,
    ));
}

fn show_flash(
    local: Query<Option<&Flashed>, With<LocalPlayer>>,
    mut overlay: Single<&mut BackgroundColor, With<FlashOverlay>>,
) {
    let white = local.single().ok().flatten().map_or(0.0, |f| f.whiteness());
    let wanted = Color::srgba(1.0, 1.0, 0.97, white.powf(0.7));
    if overlay.0 != wanted {
        overlay.0 = wanted;
    }
}

/// Muffle: the flag.
fn muffle_hearing(local: Query<Option<&Flashed>, With<LocalPlayer>>, mut hearing: ResMut<HearingMuffled>) {
    let amount = local
        .single()
        .ok()
        .flatten()
        .map_or(0.0, |f| if f.deaf_max > 0.0 { (f.deaf / (f.deaf_max * 0.5)).min(1.0) } else { 0.0 });
    if (hearing.amount - amount).abs() > 1e-3 {
        hearing.amount = amount;
    }
}

/// The global volume follows the muffle, as a bridge (only affects new
/// sounds; our sounds are all short). Client build only; a headless server
/// has no audio (no `GlobalVolume`).
#[cfg(feature = "client")]
fn lower_volume(
    hearing: Res<HearingMuffled>,
    global: Option<ResMut<GlobalVolume>>,
    mut normal: Local<Option<Volume>>,
) {
    let Some(mut global) = global else { return };
    let amount = hearing.amount;
    let base = *normal.get_or_insert(global.volume);
    let wanted = Volume::Linear(base.to_linear() * (1.0 - 0.85 * amount));
    if amount <= 0.0 {
        if global.volume != base {
            global.volume = base;
        }
        *normal = None;
    } else if global.volume != wanted {
        global.volume = wanted;
    }
}

/// A bright point light that fades out in `life` seconds.
#[derive(Component, Clone, Copy, Debug)]
#[require(Visibility)]
pub struct LightFlash {
    intensity: f32,
    life: f32,
    age: f32,
}

impl LightFlash {
    pub fn new(intensity: f32, life: f32) -> Self {
        Self {
            intensity,
            life,
            age: 0.0,
        }
    }
}

fn fade_lights(
    mut flashes: Query<(Entity, &mut LightFlash, Option<&mut PointLight>)>,
    time: Res<Time>,
    mut commands: Commands,
) {
    for (entity, mut flash, light) in &mut flashes {
        flash.age += time.delta_secs();
        let f = 1.0 - flash.age / flash.life;
        if f <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let intensity = flash.intensity * f * f;
        match light {
            Some(mut light) => light.intensity = intensity,
            None => {
                commands.entity(entity).insert(PointLight {
                    intensity,
                    range: 14.0,
                    color: Color::srgb(1.0, 0.85, 0.65),
                    shadow_maps_enabled: false,
                    ..default()
                });
            }
        }
    }
}

/// The player's view shakes near a working cutter (after the camera has
/// been placed this frame).
fn shake_near_cutters(
    cutters: Query<(&RingCutter, &GlobalTransform)>,
    local: Query<&Transform, (With<LocalPlayer>, Without<Camera3d>)>,
    mut cameras: Query<(&Camera, &mut Transform), (With<Camera3d>, Without<ChildOf>, Without<RemoteViewCamera>, Without<LocalPlayer>)>,
    tuning: Res<Tuning>,
    time: Res<Time>,
) {
    let Ok(me) = local.single() else { return };
    let radius = tuning.round.gadgets.ring_cutter.shake_radius;
    let strength = cutters
        .iter()
        .filter(|(c, _)| c.state == CutterState::Cutting)
        .map(|(_, at)| 1.0 - at.translation().distance(me.translation) / radius.max(0.1))
        .fold(0.0_f32, f32::max);
    if strength <= 0.0 {
        return;
    }
    let t = time.elapsed_secs() * 47.0;
    let offset = Vec3::new((t * 1.1).sin(), (t * 1.7).cos(), 0.0) * 0.006 * strength;
    let roll = (t * 0.9).sin() * 0.004 * strength;
    for (camera, mut transform) in &mut cameras {
        if camera.is_active {
            transform.translation += offset;
            transform.rotation *= Quat::from_rotation_z(roll);
        }
    }
}
