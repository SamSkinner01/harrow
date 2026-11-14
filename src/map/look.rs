//! Look and feel of the 3D views (client only): HDR with eye adaptation,
//! tonemapping and colour grading, bloom, SSAO and contact shadows,
//! anti-aliasing, distance haze, the HDRI sky, and ambient light that is
//! bright outdoors and dim indoors.
//!
//! Light model (no baked GI):
//! - Direct: the sun (shadow cascades, contact shadows) and the lanterns.
//! - Outdoors: a sky-fill environment light on each world camera (a
//!   hemisphere: sky above, horizon, warm sand bounce below), darkened in
//!   creases by SSAO.
//! - Indoors: a light probe box per building with a much dimmer, warmer
//!   fill ([`interior_volumes`]); inside a box it replaces the sky fill. The
//!   boxes end on the outer walls' centre lines, so outside faces stay
//!   sky-lit and inside faces get the interior fill.
//! - `GlobalAmbientLight` is only a small floor everywhere.
//!
//! Eye adaptation only ever brightens (by up to `adaptation_max` stops) in
//! dark views; outdoors keeps the sunlight exposure, so looking out of a
//! dark room the yard blooms. The haze is exponential-squared (nothing
//! within a room, thick only far out in the desert) and its colour is the
//! sky's horizon at the current exposure, so the far desert melts into the
//! sky.
//!
//! The weapon camera (render layer 1, LDR, see `weapons/viewmodel.rs`) gets
//! the same tonemapper, grading and sky fill, but none of the HDR effects.
//!
//! Everything is tuned in the `lighting` section of the tuning file;
//! per-player quality switches come from [`GraphicsOptions`] (the settings
//! screen).

use bevy::{
    anti_alias::{
        fxaa::Fxaa,
        smaa::{Smaa, SmaaPreset},
        taa::TemporalAntiAliasing,
    },
    camera::{Exposure, Hdr, visibility::RenderLayers},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    curve::cubic_splines::LinearSpline,
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, Skybox},
    pbr::{ContactShadows, ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel},
    post_process::{
        auto_exposure::{AutoExposure, AutoExposureCompensationCurve, AutoExposurePlugin},
        bloom::{Bloom, BloomCompositeMode, BloomPrefilter},
    },
    prelude::*,
    render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
};

use super::MapChoice;
use crate::config::{LightingTuning, Tonemapper, Tuning};

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(AutoExposurePlugin)
            .init_resource::<GraphicsOptions>()
            .add_systems(Startup, (load_sky, spawn_interior_probes).chain())
            .add_systems(
                PreUpdate,
                (
                    apply_look_tuning.run_if(resource_changed::<Tuning>.or_eager(resource_added::<LookAssets>)),
                    apply_graphics_options
                        .run_if(resource_changed::<GraphicsOptions>.or_eager(resource_changed::<Tuning>)),
                ),
            )
            .add_systems(PostUpdate, (configure_new_cameras, reset_taa_on_switch, nearest_interiors))
            .add_observer(mark_new_camera);
    }
}

/// Quality switches chosen by the player (settings screen). Defaults are
/// the "High" preset.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct GraphicsOptions {
    pub anti_aliasing: AntiAliasing,
    /// Screen-space ambient occlusion (`None` = off).
    pub ssao: Option<SsaoQuality>,
    pub bloom: bool,
    /// Screen-space contact shadows for the sun.
    pub contact_shadows: bool,
    pub shadows: ShadowQuality,
}

impl Default for GraphicsOptions {
    fn default() -> Self {
        Self {
            anti_aliasing: AntiAliasing::Taa,
            ssao: Some(SsaoQuality::Medium),
            bloom: true,
            contact_shadows: true,
            shadows: ShadowQuality::High,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AntiAliasing {
    Off,
    Fxaa,
    Smaa,
    /// Temporal: smoothest edges and SSAO, slight blur in motion.
    Taa,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SsaoQuality {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowQuality {
    Low,
    Medium,
    High,
    Ultra,
}

impl ShadowQuality {
    /// Sun shadow map size, cascades, shadowed distance (m); lantern shadows
    /// reach (multiplier on `lighting.lamp_shadow_distance`).
    pub fn params(self) -> (usize, usize, f32, f32) {
        match self {
            ShadowQuality::Low => (1024, 2, 60.0, 0.0),
            ShadowQuality::Medium => (2048, 3, 80.0, 0.75),
            ShadowQuality::High => (2048, 4, 100.0, 1.0),
            ShadowQuality::Ultra => (4096, 4, 120.0, 1.25),
        }
    }
}

/// The sky cubemap and the fill-light cubemaps.
#[derive(Resource)]
struct LookAssets {
    sky: Handle<Image>,
    /// `lighting.sky` the image was loaded from.
    sky_path: String,
    sky_fill: EnvironmentMapLight,
    interior_fill: EnvironmentMapLight,
}

/// A 3D camera that still needs its look (set when `Camera3d` is added,
/// handled in `PostUpdate` once the whole spawn bundle is there).
#[derive(Component)]
struct NeedsLook;

/// A camera configured as a world view (full post-processing).
#[derive(Component)]
pub struct WorldView;

/// The LDR weapon camera (render layer 1).
#[derive(Component)]
struct OverlayView;

/// Interior fill-light volumes.
#[derive(Component)]
struct InteriorProbe;

/// Bevy uses at most 8 light probes per view (`MAX_VIEW_LIGHT_PROBES`).
const MAX_INTERIOR_PROBES: usize = 8;

fn load_sky(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    tuning: Res<Tuning>,
) {
    let l = &tuning.lighting;
    let (up, side, down) = l.sky_fill_colours;
    let (iup, iside, idown) = l.interior_fill_colours;
    let rgb = |c: (f32, f32, f32)| Color::linear_rgb(c.0, c.1, c.2);
    commands.insert_resource(LookAssets {
        sky: assets.load(l.sky.clone()),
        sky_path: l.sky.clone(),
        sky_fill: EnvironmentMapLight::hemispherical_gradient(&mut images, rgb(up), rgb(side), rgb(down))
            .with_intensity(l.sky_fill),
        interior_fill: EnvironmentMapLight::hemispherical_gradient(&mut images, rgb(iup), rgb(iside), rgb(idown))
            .with_intensity(l.interior_fill),
    });
}

/// Building interiors that get the dim fill instead of the sky, per map:
/// (min, max) corners, world metres. Boxes run to the outer walls' centre
/// lines, from just under the ground floor to just under the roof's top.
fn interior_volumes(map: MapChoice) -> Vec<(Vec3, Vec3)> {
    let b = |x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32| (Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1));
    match map {
        MapChoice::Keep => {
            let mut v = vec![
                // The keep, basement to the second floor (the terrace on
                // its roof stays outdoors), and the third-floor block.
                b(-16.0, -3.5, -30.0, 16.0, 6.3, -8.0),
                b(-8.0, 6.3, -30.0, 8.0, 9.5, -18.0),
                // Barracks and granary (two storeys; roofs carry the walk).
                b(-40.0, -0.3, -20.0, -28.0, 6.3, 4.0),
                b(28.0, -0.3, -20.0, 40.0, 6.3, 4.0),
                // Old hall (one storey) and the gatehouse with its passage.
                b(-30.0, -0.3, 12.0, -14.0, 3.1, 24.0),
                b(-6.0, -0.3, 26.0, 6.0, 6.3, 34.0),
            ];
            // Corner towers, 8 x 8 m, up to the roof.
            for (x, z) in [(-42.0, -32.0), (34.0, -32.0), (-42.0, 24.0), (34.0, 24.0)] {
                v.push(b(x, -0.3, z, x + 8.0, 9.5, z + 8.0));
            }
            v
        }
        MapChoice::SectionA => vec![b(0.0, -0.3, -8.0, 12.0, 6.3, 0.0)],
        MapChoice::Greybox => Vec::new(),
    }
}

fn spawn_interior_probes(mut commands: Commands, assets: Res<LookAssets>) {
    for (min, max) in interior_volumes(MapChoice::from_args()) {
        commands.spawn((
            Name::new("Interior fill"),
            InteriorProbe,
            LightProbe::new(),
            assets.interior_fill.clone(),
            Transform::from_translation((min + max) / 2.0).with_scale(max - min),
            // Also lights the weapon in your hands (its camera draws layer 1).
            RenderLayers::from_layers(&[0, 1]),
        ));
    }
}

fn mark_new_camera(add: On<Add<Camera3d>>, mut commands: Commands) {
    commands.entity(add.entity).insert(NeedsLook);
}

/// Linear RGB from a tuning triple.
fn linear(c: (f32, f32, f32)) -> Color {
    Color::linear_rgb(c.0, c.1, c.2)
}

fn tonemapping(t: Tonemapper) -> Tonemapping {
    match t {
        Tonemapper::AgX => Tonemapping::AgX,
        Tonemapper::TonyMcMapface => Tonemapping::TonyMcMapface,
        Tonemapper::AcesFitted => Tonemapping::AcesFitted,
        Tonemapper::BlenderFilmic => Tonemapping::BlenderFilmic,
        Tonemapper::KhronosPbrNeutral => Tonemapping::KhronosPbrNeutral,
    }
}

fn color_grading(l: &LightingTuning) -> ColorGrading {
    let g = &l.grading;
    // No contrast or lift: Bevy applies them around 0.5 in linear light
    // before eye adaptation, which crushed dark interiors to black.
    let section = |saturation: f32, gain: f32| ColorGradingSection {
        saturation,
        contrast: 1.0,
        gamma: 1.0,
        gain,
        lift: 0.0,
    };
    ColorGrading {
        global: ColorGradingGlobal {
            exposure: 0.0,
            temperature: g.temperature,
            tint: g.tint,
            post_saturation: g.post_saturation,
            ..default()
        },
        shadows: section(g.saturation * g.shadow_saturation, 1.0),
        midtones: section(g.saturation, 1.0),
        highlights: section(g.saturation * g.highlight_saturation, g.highlight_gain),
    }
}

/// The haze colour: the sky's horizon at the base exposure (fog is mixed
/// in after exposure, the sky before it).
fn fog(l: &LightingTuning) -> DistanceFog {
    let exposure = Exposure { ev100: l.exposure }.exposure();
    let (r, g, b) = l.fog_colour;
    let k = l.sky_brightness * exposure;
    let (sr, sg, sb) = l.sun_colour;
    DistanceFog {
        color: Color::linear_rgb(r * k, g * k, b * k),
        directional_light_color: Color::linear_rgba(sr, sg, sb, l.fog_sun_glow),
        directional_light_exponent: 24.0,
        falloff: FogFalloff::from_visibility_squared(l.fog_visibility),
    }
}

/// Eye adaptation that only ever brightens, and by at most `max_boost`
/// stops. Bevy's auto exposure targets `curve(avg) - avg` stops of
/// correction on top of the base exposure (avg = average log2 luminance),
/// so: at or above `threshold` the curve is y = x (no change); just below it
/// the curve is flat (brighten towards the threshold); far below it the
/// curve is y = x + max_boost (capped boost).
fn adaptation_curve(threshold: f32, max_boost: f32) -> AutoExposureCompensationCurve {
    let boost = max_boost.max(0.01);
    let (lo, hi) = (-16.0, 8.0);
    let points = [
        Vec2::new(lo, lo + boost),
        Vec2::new(threshold - boost, threshold),
        Vec2::new(threshold, threshold),
        Vec2::new(hi, hi),
    ];
    AutoExposureCompensationCurve::from_curve(LinearSpline::new(points))
        .expect("adaptation curve is monotonic")
}

fn auto_exposure(l: &LightingTuning, curves: &mut Assets<AutoExposureCompensationCurve>) -> AutoExposure {
    AutoExposure {
        range: -16.0..=8.0,
        // Ignore the darkest 30% (black corners) and the brightest 5% (the
        // sun, lamp glass) when metering.
        filter: 0.30..=0.95,
        speed_brighten: l.adaptation_speed.0,
        speed_darken: l.adaptation_speed.1,
        compensation_curve: curves.add(adaptation_curve(l.adaptation_threshold, l.adaptation_max)),
        ..default()
    }
}

fn skybox(l: &LightingTuning, assets: &LookAssets) -> Skybox {
    Skybox {
        image: Some(assets.sky.clone()),
        brightness: l.sky_brightness,
        // The cubemap has the sun due south; turn it to the sun's bearing.
        rotation: Quat::from_rotation_y((180.0 - l.sun_azimuth).to_radians()),
    }
}

/// Give new 3D cameras their look: world views get the full HDR pipeline,
/// the weapon overlay only matching colours and fill light.
#[allow(clippy::type_complexity)]
fn configure_new_cameras(
    mut cameras: Query<(Entity, Option<&RenderLayers>, Option<&mut Projection>), With<NeedsLook>>,
    assets: Option<Res<LookAssets>>,
    tuning: Res<Tuning>,
    options: Res<GraphicsOptions>,
    mut curves: ResMut<Assets<AutoExposureCompensationCurve>>,
    mut commands: Commands,
) {
    let Some(assets) = assets else {
        return;
    };
    let l = &tuning.lighting;
    for (entity, layers, projection) in &mut cameras {
        let mut camera = commands.entity(entity);
        camera.remove::<NeedsLook>();
        let overlay = layers.is_some_and(|layers| !layers.intersects(&RenderLayers::layer(0)));
        let shared = (tonemapping(l.tonemapper), color_grading(l), DebandDither::Enabled);
        if overlay {
            // The weapon is mostly dark metal right in front of the eye:
            // a little extra fill keeps it from reading as a silhouette.
            let fill = assets.sky_fill.clone().with_intensity(l.sky_fill * l.weapon_fill);
            camera.insert((OverlayView, shared, fill));
            continue;
        }
        // See the desert floor out to its edge on the horizon (3 km; the
        // projection is infinite, far only culls).
        if let Some(mut projection) = projection
            && let Projection::Perspective(p) = projection.as_mut()
        {
            p.far = p.far.max(4000.0);
        }
        camera.insert((
            WorldView,
            shared,
            // Sky fill: the camera's own environment light, used wherever
            // no interior probe contains the surface.
            assets.sky_fill.clone(),
            Hdr,
            Exposure { ev100: l.exposure },
            auto_exposure(l, &mut curves),
            fog(l),
            skybox(l, &assets),
        ));
        apply_camera_options(&mut camera, &options, l);
    }
}

/// Anti-aliasing, SSAO, bloom and contact shadows on one world camera.
fn apply_camera_options(camera: &mut EntityCommands, options: &GraphicsOptions, l: &LightingTuning) {
    camera.insert(Msaa::Off);
    camera.remove::<(Fxaa, Smaa, TemporalAntiAliasing)>();
    match options.anti_aliasing {
        AntiAliasing::Off => {}
        AntiAliasing::Fxaa => {
            camera.insert(Fxaa::default());
        }
        AntiAliasing::Smaa => {
            camera.insert(Smaa {
                preset: SmaaPreset::High,
            });
        }
        AntiAliasing::Taa => {
            camera.insert(TemporalAntiAliasing::default());
        }
    }
    match options.ssao {
        Some(quality) => {
            camera.insert(ScreenSpaceAmbientOcclusion {
                quality_level: match quality {
                    SsaoQuality::Low => ScreenSpaceAmbientOcclusionQualityLevel::Low,
                    SsaoQuality::Medium => ScreenSpaceAmbientOcclusionQualityLevel::Medium,
                    SsaoQuality::High => ScreenSpaceAmbientOcclusionQualityLevel::High,
                },
                radius: l.ssao_radius,
                constant_object_thickness: l.ssao_thickness,
            });
        }
        None => {
            camera.remove::<ScreenSpaceAmbientOcclusion>();
        }
    }
    if options.bloom && l.bloom > 0.0 {
        camera.insert(Bloom {
            intensity: l.bloom,
            prefilter: BloomPrefilter {
                threshold: l.bloom_threshold,
                threshold_softness: 0.6,
            },
            composite_mode: BloomCompositeMode::Additive,
            ..Bloom::NATURAL
        });
    } else {
        camera.remove::<Bloom>();
    }
    if options.contact_shadows {
        camera.insert(ContactShadows {
            linear_steps: 12,
            thickness: 0.15,
            length: 0.5,
        });
    } else {
        camera.remove::<ContactShadows>();
    }
}

/// Tuning changed: exposure, adaptation, grading, haze, sky and fill
/// lights on every configured camera.
#[allow(clippy::type_complexity)]
fn apply_look_tuning(
    tuning: Res<Tuning>,
    assets: Option<ResMut<LookAssets>>,
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut curves: ResMut<Assets<AutoExposureCompensationCurve>>,
    mut world: Query<
        (
            &mut Exposure,
            &mut AutoExposure,
            &mut DistanceFog,
            &mut Skybox,
            &mut Tonemapping,
            &mut ColorGrading,
        ),
        With<WorldView>,
    >,
    mut overlays: Query<(&mut Tonemapping, &mut ColorGrading), (With<OverlayView>, Without<WorldView>)>,
    mut fills: Query<(&mut EnvironmentMapLight, Has<OverlayView>), (With<Camera3d>, Without<InteriorProbe>)>,
    mut probes: Query<&mut EnvironmentMapLight, With<InteriorProbe>>,
) {
    let Some(mut assets) = assets else {
        return;
    };
    let l = &tuning.lighting;
    // A different sky in the tuning file.
    if assets.sky_path != l.sky {
        assets.sky = server.load(l.sky.clone());
        assets.sky_path = l.sky.clone();
    }
    // Rebuild the fill cubemaps (cheap: 1x1 faces).
    let (up, side, down) = l.sky_fill_colours;
    let (iup, iside, idown) = l.interior_fill_colours;
    let sky_fill = EnvironmentMapLight::hemispherical_gradient(&mut images, linear(up), linear(side), linear(down))
        .with_intensity(l.sky_fill);
    let interior_fill =
        EnvironmentMapLight::hemispherical_gradient(&mut images, linear(iup), linear(iside), linear(idown))
            .with_intensity(l.interior_fill);
    for (mut fill, overlay) in &mut fills {
        let boost = if overlay { l.weapon_fill } else { 1.0 };
        *fill = sky_fill.clone().with_intensity(l.sky_fill * boost);
    }
    for mut probe in &mut probes {
        *probe = interior_fill.clone();
    }
    let curve = curves.add(adaptation_curve(l.adaptation_threshold, l.adaptation_max));
    for (mut exposure, mut auto, mut haze, mut sky, mut tonemap, mut grading) in &mut world {
        exposure.ev100 = l.exposure;
        auto.compensation_curve = curve.clone();
        auto.speed_brighten = l.adaptation_speed.0;
        auto.speed_darken = l.adaptation_speed.1;
        *haze = fog(l);
        *sky = skybox(l, &assets);
        *tonemap = tonemapping(l.tonemapper);
        *grading = color_grading(l);
    }
    for (mut tonemap, mut grading) in &mut overlays {
        *tonemap = tonemapping(l.tonemapper);
        *grading = color_grading(l);
    }
    assets.sky_fill = sky_fill;
    assets.interior_fill = interior_fill;
}

/// Quality switches: per-camera effects, the sun's shadow cascades and the
/// lanterns' shadow reach.
fn apply_graphics_options(
    options: Res<GraphicsOptions>,
    tuning: Res<Tuning>,
    cameras: Query<Entity, With<WorldView>>,
    mut suns: Query<(&mut DirectionalLight, &mut bevy::light::CascadeShadowConfig)>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    mut lamp_reach: ResMut<super::lamps::LampShadowScale>,
    mut commands: Commands,
) {
    let l = &tuning.lighting;
    for camera in &cameras {
        apply_camera_options(&mut commands.entity(camera), &options, l);
    }
    let (size, cascades, distance, lamps) = options.shadows.params();
    if shadow_map.size != size {
        shadow_map.size = size;
    }
    lamp_reach.0 = lamps;
    for (mut sun, mut config) in &mut suns {
        sun.contact_shadows_enabled = options.contact_shadows;
        *config = CascadeShadowConfigBuilder {
            num_cascades: cascades,
            first_cascade_far_bound: 8.0,
            maximum_distance: distance,
            ..default()
        }
        .build();
    }
}

/// TAA keeps a history of past frames; when a camera comes back on (drone
/// or CCTV view ends) or cuts (menu backdrop to the player), start afresh
/// instead of smearing the old picture into the new one.
fn reset_taa_on_switch(
    mut cameras: Query<(Entity, &Camera, &mut TemporalAntiAliasing)>,
    state: Res<State<crate::session::AppState>>,
    mut was_active: Local<bevy::platform::collections::HashMap<Entity, bool>>,
) {
    let cut = state.is_changed();
    for (entity, camera, mut taa) in &mut cameras {
        let before = was_active.insert(entity, camera.is_active).unwrap_or(false);
        if camera.is_active && (!before || cut) {
            taa.reset = true;
        }
    }
}

/// Only the interior probes nearest the active camera (by distance to the
/// box) stay on: Bevy keeps the 8 nearest by centre, which can drop the
/// one you stand in for a smaller one nearby. Far interiors (seen through a
/// window across the compound) then get the sky fill; near ones are always
/// right.
fn nearest_interiors(
    cameras: Query<(&Camera, &GlobalTransform), With<WorldView>>,
    mut probes: Query<(&GlobalTransform, &mut Visibility), With<InteriorProbe>>,
) {
    let Some(eye) = cameras.iter().find(|(c, _)| c.is_active).map(|(_, t)| t.translation()) else {
        return;
    };
    let mut by_distance: Vec<(f32, usize)> = probes
        .iter()
        .enumerate()
        .map(|(i, (at, _))| {
            // Distance from the eye to the box (0 inside).
            let (scale, _, centre) = at.to_scale_rotation_translation();
            let outside = ((eye - centre).abs() - scale / 2.0).max(Vec3::ZERO);
            (outside.length(), i)
        })
        .collect();
    by_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
    let keep: Vec<usize> = by_distance.iter().take(MAX_INTERIOR_PROBES).map(|(_, i)| *i).collect();
    for (i, (_, mut visibility)) in probes.iter_mut().enumerate() {
        let wanted = if keep.contains(&i) { Visibility::Inherited } else { Visibility::Hidden };
        visibility.set_if_neq(wanted);
    }
}
