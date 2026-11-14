//! What a world collider sounds like, for footsteps and bullet impacts.
//!
//! In order:
//! 1. Destructible surfaces (`SoftSurface`) know their material: plaster or
//!    wood skin, metal on a hatch frame (fixture) or where a reinforcement
//!    plate faces the hit.
//! 2. Otherwise the names of the collider, its ancestors and its rigid body
//!    (kit pieces keep their piece name, e.g. `floor_wood_2m`, on the root)
//!    are matched against `audio.surfaces` in the tuning file.
//! 3. Otherwise `audio.default_surface`.

use avian3d::prelude::*;
use bevy::{ecs::system::SystemParam, prelude::*};

use crate::{
    config::{SoundMaterial, Tuning},
    destruction::{SoftSurface, SurfaceMaterial},
};

/// The material of a hit and what decided it (for `--log-sounds`).
pub struct SurfaceHit {
    pub material: SoundMaterial,
    pub source: String,
}

impl SurfaceHit {
    /// "Wood (floor_wood_2m)".
    pub fn describe(&self) -> String {
        format!("{:?} ({})", self.material, self.source)
    }
}

#[derive(SystemParam)]
pub struct SurfaceLookup<'w, 's> {
    colliders: Query<'w, 's, &'static ColliderOf>,
    names: Query<'w, 's, &'static Name>,
    parents: Query<'w, 's, &'static ChildOf>,
    soft: Query<'w, 's, (&'static SoftSurface, &'static GlobalTransform)>,
    tuning: Res<'w, Tuning>,
}

impl SurfaceLookup<'_, '_> {
    /// Material of `collider`, hit at `point`; `from` is a point just off
    /// the surface on the side the hit came from.
    pub fn material(&self, collider: Entity, point: Vec3, from: Vec3) -> SurfaceHit {
        let body = self.colliders.get(collider).map_or(collider, |c| c.body);
        if let Ok((soft, transform)) = self.soft.get(body) {
            return SurfaceHit {
                material: soft_material(soft, transform, point, from),
                source: format!("{}, destructible", self.name(body)),
            };
        }
        // The collider, up its hierarchy (COL_* node -> kit piece root), then
        // the body.
        let mut entity = Some(collider);
        for _ in 0..8 {
            let Some(e) = entity else { break };
            if let Some(hit) = self.by_name(e) {
                return hit;
            }
            entity = self.parents.get(e).ok().map(ChildOf::parent);
        }
        if let Some(hit) = self.by_name(body) {
            return hit;
        }
        SurfaceHit {
            material: self.tuning.audio.default_surface,
            source: format!("{}, default", self.name(body)),
        }
    }

    /// Nothing under the ray (mid-air, off the map).
    pub fn fallback(&self) -> SurfaceHit {
        SurfaceHit {
            material: self.tuning.audio.default_surface,
            source: "nothing below, default".into(),
        }
    }

    fn name(&self, entity: Entity) -> String {
        self.names
            .get(entity)
            .map_or_else(|_| format!("{entity}"), |n| n.as_str().to_string())
    }

    fn by_name(&self, entity: Entity) -> Option<SurfaceHit> {
        let name = self.names.get(entity).ok()?;
        let material = material_for_name(name.as_str(), &self.tuning.audio.surfaces)?;
        Some(SurfaceHit {
            material,
            source: name.as_str().to_string(),
        })
    }
}

/// First pattern (case-insensitive) contained in `name`.
pub fn material_for_name(name: &str, table: &[(String, SoundMaterial)]) -> Option<SoundMaterial> {
    let name = name.to_ascii_lowercase();
    table
        .iter()
        .find(|(pattern, _)| name.contains(&pattern.to_ascii_lowercase()))
        .map(|(_, material)| *material)
}

pub fn from_surface_material(material: SurfaceMaterial) -> SoundMaterial {
    match material {
        SurfaceMaterial::Plaster => SoundMaterial::Plaster,
        SurfaceMaterial::Wood => SoundMaterial::Wood,
        SurfaceMaterial::Metal => SoundMaterial::Metal,
    }
}

fn soft_material(
    soft: &SoftSurface,
    transform: &GlobalTransform,
    point: Vec3,
    from: Vec3,
) -> SoundMaterial {
    let local = transform.affine().inverse().transform_point3(point);
    let margin = Vec3::splat(0.03);
    if let Some(fixture) = soft.spec.fixtures.iter().find(|f| {
        f.present && local.cmpge(f.min - margin).all() && local.cmple(f.max + margin).all()
    }) {
        return from_surface_material(fixture.material);
    }
    if let Some(plate) = &soft.reinforced
        && soft.side_of(transform, from) == plate.side
    {
        return SoundMaterial::Metal;
    }
    from_surface_material(soft.spec.skin_material)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_matching_pattern_wins() {
        let table = vec![
            ("hatch".to_string(), SoundMaterial::Wood),
            ("sand".to_string(), SoundMaterial::Sand),
            ("stone".to_string(), SoundMaterial::Stone),
        ];
        assert_eq!(
            material_for_name("floor_hatch_2m", &table),
            Some(SoundMaterial::Wood)
        );
        assert_eq!(
            material_for_name("Desert SAND", &table),
            Some(SoundMaterial::Sand)
        );
        assert_eq!(
            material_for_name("floor_stone_2m", &table),
            Some(SoundMaterial::Stone)
        );
        assert_eq!(material_for_name("merlon", &table), None);
    }
}
