//! The ground row of the scattering table already holds reflected radiance,
//! including filtered direct sunlight, sky light and the surface albedo. Share
//! one frame's decision between both surface materials and the shell, so the
//! shell never dims an already atmosphere-lit surface a second time.
use super::{
    Shell,
    tables::{AtmosphereTables, Tables},
};
use crate::render::{MAX_LIGHTS, debug::RenderDebug, resolve::Frame};
use bevy::{prelude::*, render::render_resource::ShaderType};

#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct SurfaceUniform {
    /// World pole and equatorial/polar stretch; zero disables table lighting.
    pub pole: Vec4,
    /// Point/disc presentation blend; x is atmospheric lighting's share.
    pub blend: Vec4,
    /// Exposed stellar flux, not reflected radiance: the table includes albedo.
    pub suns: [Vec4; MAX_LIGHTS],
}

#[derive(Clone, Default, PartialEq)]
pub struct SurfaceLight {
    pub uniform: SurfaceUniform,
    pub table: Option<Handle<Image>>,
}

impl SurfaceLight {
    fn new(shell: &Shell, debug: &RenderDebug, tables: Option<Tables>) -> Self {
        let table = tables
            .filter(|_| debug.atmospheres && debug.atmosphere_tables && debug.atmosphere_ground)
            .and_then(|t| t.multiple)
            .map(|m| m.sky);
        if table.is_none() {
            return Self::default();
        }
        Self {
            uniform: SurfaceUniform {
                pole: shell.pole.extend(1.0 / (1.0 - shell.flattening)),
                blend: Vec4::splat(shell.fade),
                suns: shell.suns,
            },
            table,
        }
    }
}

/// Indexed by `Frame::shells`, also referenced by `Shaded::atmosphere`.
#[derive(Resource, Default)]
pub struct SurfaceLighting(pub Vec<SurfaceLight>);

impl SurfaceLighting {
    pub fn for_body(&self, atmosphere: Option<usize>) -> SurfaceLight {
        atmosphere
            .and_then(|index| self.0.get(index))
            .cloned()
            .unwrap_or_default()
    }
}

pub fn prepare_surface_lighting(
    frame: Res<Frame>,
    debug: Res<RenderDebug>,
    mut tables: ResMut<AtmosphereTables>,
    mut lighting: ResMut<SurfaceLighting>,
) {
    lighting.0 = frame
        .shells
        .iter()
        .map(|shell| {
            let tables = (debug.atmospheres && debug.atmosphere_tables)
                .then(|| tables.get(&shell.air))
                .flatten();
            SurfaceLight::new(shell, &debug, tables)
        })
        .collect();
}

#[cfg(test)]
mod tests;
