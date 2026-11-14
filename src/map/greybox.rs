//! Grey-box test area for the player controller.
//!
//! Everything is built from axis-aligned boxes with a 1 m checker, so speeds,
//! heights and distances are easy to judge. Contents:
//!
//! - A one-storey building (3 m walls, roof) with doors, windows, an interior
//!   wall with a doorway, and corners to lean around.
//! - Courtyard pillars and a waist-high wall.
//! - A crouch tunnel (1.4 m ceiling) and a crawlspace (0.8 m ceiling).
//! - Stairs up to a railed platform 3 m high, with a drop-off.
//!
//! Stairs use an invisible ramp collider under visual steps; the character
//! controller does not step up ledges, so every staircase needs a ramp.

use avian3d::prelude::*;
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::VertexAttributeValues,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

pub struct GreyboxPlugin;

impl Plugin for GreyboxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_greybox);
    }
}

/// Wall height and thickness for the test building.
const H: f32 = 3.0;
const T: f32 = 0.3;

fn spawn_greybox(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let checker = images.add(checker_image());
    let mut material = |r: f32, g: f32, b: f32| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            base_color_texture: Some(checker.clone()),
            perceptual_roughness: 0.95,
            ..default()
        })
    };
    let floor = material(0.62, 0.60, 0.57);
    let wall = material(0.80, 0.78, 0.74);
    let prop = material(0.85, 0.55, 0.25);
    let stone = material(0.56, 0.54, 0.51);
    let stairs = material(0.70, 0.68, 0.64);

    let mut b = Builder {
        commands: &mut commands,
        meshes: &mut meshes,
    };

    // Ground pad with a 1 m grid. Its top is y = 0.
    b.solid(&floor, [-30.0, -0.1, -30.0], [30.0, 0.0, 30.0]);

    // --- Building: x -8..8, z -10..0 ---------------------------------------
    // South wall: main door (1.2 x 2.2 m) and a window (sill 1.0, top 2.1).
    b.solid(&wall, [-8.0, 0.0, -T], [-0.6, H, 0.0]);
    b.solid(&wall, [0.6, 0.0, -T], [3.0, H, 0.0]);
    b.solid(&wall, [4.6, 0.0, -T], [8.0, H, 0.0]);
    b.solid(&wall, [-0.6, 2.2, -T], [0.6, H, 0.0]);
    b.solid(&wall, [3.0, 0.0, -T], [4.6, 1.0, 0.0]);
    b.solid(&wall, [3.0, 2.1, -T], [4.6, H, 0.0]);
    // North wall.
    b.solid(&wall, [-8.0, 0.0, -10.0], [8.0, H, -10.0 + T]);
    // West wall with a window.
    b.solid(&wall, [-8.0, 0.0, -10.0 + T], [-8.0 + T, H, -6.0]);
    b.solid(&wall, [-8.0, 0.0, -4.5], [-8.0 + T, H, -T]);
    b.solid(&wall, [-8.0, 0.0, -6.0], [-8.0 + T, 1.0, -4.5]);
    b.solid(&wall, [-8.0, 2.1, -6.0], [-8.0 + T, H, -4.5]);
    // East wall with a side door.
    b.solid(&wall, [8.0 - T, 0.0, -10.0 + T], [8.0, H, -3.2]);
    b.solid(&wall, [8.0 - T, 0.0, -2.0], [8.0, H, -T]);
    b.solid(&wall, [8.0 - T, 2.2, -3.2], [8.0, H, -2.0]);
    // Interior wall splitting the back rooms, with a doorway. It stops short
    // of the front hall, leaving a corner to lean around.
    b.solid(&wall, [-0.15, 0.0, -10.0 + T], [0.15, H, -7.2]);
    b.solid(&wall, [-0.15, 0.0, -6.0], [0.15, H, -3.5]);
    b.solid(&wall, [-0.15, 2.2, -7.2], [0.15, H, -6.0]);
    // Roof.
    b.solid(&wall, [-8.0, H, -10.0], [8.0, H + 0.25, 0.0]);
    // Cover inside.
    b.solid(&prop, [-5.0, 0.0, -8.5], [-3.8, 1.2, -7.3]);
    b.solid(&prop, [3.0, 0.0, -5.0], [4.0, 1.0, -4.0]);
    b.solid(&prop, [5.5, 0.0, -8.8], [7.2, 1.6, -8.0]);

    // --- Courtyard -----------------------------------------------------------
    b.solid(&stone, [-6.4, 0.0, 5.6], [-5.6, H, 6.4]);
    b.solid(&stone, [5.6, 0.0, 5.6], [6.4, H, 6.4]);
    b.solid(&stone, [-3.0, 0.0, 4.0], [1.0, 1.0, 4.4]);

    // --- Crouch tunnel: 1.8 m wide, 1.4 m ceiling ----------------------------
    b.solid(&stone, [-14.0, 0.0, -6.0], [-13.7, 1.4, -2.0]);
    b.solid(&stone, [-11.9, 0.0, -6.0], [-11.6, 1.4, -2.0]);
    b.solid(&stone, [-14.0, 1.4, -6.0], [-11.6, 1.6, -2.0]);

    // --- Crawlspace: 1.8 m wide, 0.8 m ceiling -------------------------------
    b.solid(&stone, [-18.4, 0.0, -6.0], [-18.1, 0.8, -2.0]);
    b.solid(&stone, [-16.3, 0.0, -6.0], [-16.0, 0.8, -2.0]);
    b.solid(&stone, [-18.4, 0.8, -6.0], [-16.0, 1.0, -2.0]);

    // --- Stairs (x 10..12) rising 3 m over 5 m to a platform -----------------
    const STEPS: usize = 12;
    let (run, rise) = (5.0, 3.0);
    let depth = run / STEPS as f32;
    for i in 0..STEPS {
        let z_front = 2.0 - i as f32 * depth;
        let top = (i + 1) as f32 * rise / STEPS as f32;
        b.visual(&stairs, [10.0, 0.0, z_front - depth], [12.0, top, z_front]);
    }
    b.ramp([10.0, 12.0], [0.0, 2.0], [rise, 2.0 - run]);
    // Platform, top at y = 3, with supports and railings.
    b.solid(&stone, [10.0, 2.8, -7.0], [14.0, 3.0, -3.0]);
    b.solid(&stone, [13.6, 0.0, -7.0], [14.0, 2.8, -6.6]);
    b.solid(&stone, [13.6, 0.0, -3.4], [14.0, 2.8, -3.0]);
    b.solid(&stone, [10.0, 0.0, -7.0], [10.4, 2.8, -6.6]);
    b.solid(&wall, [10.0, 3.0, -7.0], [14.0, 4.0, -6.9]);
    b.solid(&wall, [13.9, 3.0, -7.0], [14.0, 4.0, -3.0]);
    b.solid(&wall, [12.0, 3.0, -3.1], [14.0, 4.0, -3.0]);
    b.solid(&wall, [10.0, 3.0, -7.0], [10.1, 4.0, -3.0]);
}

struct Builder<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    meshes: &'a mut Assets<Mesh>,
}

impl Builder<'_, '_, '_> {
    /// A visible box with a static collider, given by its min and max corners.
    fn solid(&mut self, material: &Handle<StandardMaterial>, min: [f32; 3], max: [f32; 3]) {
        let (center, size) = center_size(min, max);
        self.commands.spawn((
            Mesh3d(self.meshes.add(world_uv_box(center, size))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(center),
            RigidBody::Static,
            Collider::cuboid(size.x, size.y, size.z),
        ));
    }

    /// A visible box with no collision.
    fn visual(&mut self, material: &Handle<StandardMaterial>, min: [f32; 3], max: [f32; 3]) {
        let (center, size) = center_size(min, max);
        self.commands.spawn((
            Mesh3d(self.meshes.add(world_uv_box(center, size))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(center),
        ));
    }

    /// Invisible walkable ramp spanning `x_range`, from `(y, z)` = `low` to `high`.
    fn ramp(&mut self, x_range: [f32; 2], low: [f32; 2], high: [f32; 2]) {
        const THICKNESS: f32 = 0.1;
        let low = Vec3::new(0.0, low[0], low[1]);
        let high = Vec3::new(0.0, high[0], high[1]);
        let along = high - low;
        let length = along.length();
        // Rotate the slab's +Z axis onto the downhill direction.
        let rotation = Quat::from_rotation_arc(Vec3::Z, -along / length);
        let normal = rotation * Vec3::Y;
        let mut center = (low + high) / 2.0 - normal * (THICKNESS / 2.0);
        center.x = (x_range[0] + x_range[1]) / 2.0;
        self.commands.spawn((
            RigidBody::Static,
            Collider::cuboid(x_range[1] - x_range[0], THICKNESS, length),
            Transform::from_translation(center).with_rotation(rotation),
        ));
    }
}

fn center_size(min: [f32; 3], max: [f32; 3]) -> (Vec3, Vec3) {
    let (min, max) = (Vec3::from(min), Vec3::from(max));
    ((min + max) / 2.0, max - min)
}

/// A box mesh whose UVs are world-space metres, so the checker lines up across
/// every box and always shows 1 m squares.
fn world_uv_box(center: Vec3, size: Vec3) -> Mesh {
    let mut mesh = Cuboid::from_size(size).mesh().build();
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION).cloned()
    else {
        unreachable!("cuboid meshes always have float3 positions");
    };
    let Some(VertexAttributeValues::Float32x3(normals)) =
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL).cloned()
    else {
        unreachable!("cuboid meshes always have float3 normals");
    };
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .zip(&normals)
        .map(|(p, n)| {
            let w = Vec3::from(*p) + center;
            let n = Vec3::from(*n);
            let uv = if n.x.abs() > 0.5 {
                Vec2::new(w.z, -w.y)
            } else if n.y.abs() > 0.5 {
                Vec2::new(w.x, w.z)
            } else {
                Vec2::new(w.x, -w.y)
            };
            // The checker texture is 2x2 texels, so 0.5 UV per metre.
            (uv * 0.5).to_array()
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh
}

/// 2x2 low-contrast checker, tiled. Tinted by each material's base colour.
fn checker_image() -> Image {
    let (a, b) = (255u8, 222u8);
    #[rustfmt::skip]
    let data = vec![
        a, a, a, 255,   b, b, b, 255,
        b, b, b, 255,   a, a, a, 255,
    ];
    let mut image = Image::new(
        Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}
