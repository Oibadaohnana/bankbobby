//! The world on screen: one material, [`WorldMaterial`], for the ground and
//! for every mesh over it, and the three textures the ray tracer reads.
//!
//! The ground is one quad over the whole map, drawn by `world.wgsl` in mode
//! 0 (its floors and walls are worked out per pixel from the tile texture).
//! The furniture is three meshes — under the furniture, the furniture, and
//! over the player — and the robber a handful of small ones, all in mode 1
//! (their colours are the vertices'). Every one of them is lit and fogged
//! per pixel by the same rays, so a desk in the dark is as dark as the
//! floor under it.
//!
//! The material's parameters — where the eyes are, the lamps — change every
//! frame and are written into each material by [`update_params`].

use crate::map::{Map, Wall};
use crate::movement::TILE;
use crate::paint;
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat,
};
use bevy::shader::{Shader, ShaderRef};
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};

const WORLD_SHADER: Handle<Shader> = uuid_handle!("3f6c1b2e-8d4a-4e7b-9a51-2c7d0e9b6f41");

/// The most lamps a frame lights with: the nearest to the player.
pub const MAX_LIGHTS: usize = 128;

/// How far the player sees: lit (`VIEW_RANGE`) and in the dark
/// (`DARK_RANGE`), in tiles — bims' fourteen and eight.
pub const VIEW_RANGE: f32 = 14.0;
pub const DARK_RANGE: f32 = 8.0;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, WORLD_SHADER, "shaders/world.wgsl", Shader::from_wgsl);
        app.add_plugins(Material2dPlugin::<WorldMaterial>::default());
    }
}

#[derive(ShaderType, Clone, Debug)]
pub struct Params {
    pub eye: Vec4,
    pub map: Vec4,
    pub misc: Vec4,
    pub lights: [Vec4; 2 * MAX_LIGHTS],
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct WorldMaterial {
    #[uniform(0)]
    pub params: Params,
    #[texture(1)]
    pub tiles: Handle<Image>,
    #[texture(2)]
    pub occ: Handle<Image>,
    #[texture(3)]
    pub extra: Handle<Image>,
    /// Blended (the meshes, whose shadows are translucent) or not (the ground).
    pub blend: bool,
}

impl Material2d for WorldMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(WORLD_SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        if self.blend {
            AlphaMode2d::Blend
        } else {
            AlphaMode2d::Opaque
        }
    }

    /// No culling: a triangle is drawn whichever way round it was wound.
    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Everything spawned for one map, taken down whole for the next.
#[derive(Component)]
pub struct WorldPart;

/// The materials whose parameters follow the player.
#[derive(Resource, Default)]
pub struct WorldMaterials {
    pub all: Vec<Handle<WorldMaterial>>,
}

/// The map's lamps, packed for the shader.
#[derive(Resource, Default)]
pub struct Lamps {
    pub all: Vec<(Vec2, Vec4, Vec4)>,
}

fn grid_image(map: &Map, texel: impl Fn(i32, i32) -> [u8; 4]) -> Image {
    let mut data = Vec::with_capacity((map.w * map.h * 4) as usize);
    for y in 0..map.h {
        for x in 0..map.w {
            data.extend(texel(x, y));
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: map.w as u32,
            height: map.h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::nearest());
    image
}

fn params(map: &Map, mode: f32) -> Params {
    Params {
        eye: vec4(
            map.spawn.x,
            map.spawn.y,
            VIEW_RANGE * TILE,
            DARK_RANGE * TILE,
        ),
        map: vec4(map.w as f32, map.h as f32, TILE, 0.0),
        misc: vec4(mode, 0.0, 0.0, 0.0),
        lights: [Vec4::ZERO; 2 * MAX_LIGHTS],
    }
}

/// Spawn the map: the ground, the furniture's three layers, and the lamps.
pub fn spawn_map(
    commands: &mut Commands,
    map: &Map,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<WorldMaterial>,
    world_materials: &mut WorldMaterials,
    lamps: &mut Lamps,
) {
    let tiles = images.add(grid_image(map, |x, y| {
        let t = map.at(x, y);
        [t.floor as u8, t.wall as u8, t.tint, t.deco]
    }));
    let occluders = map.occluders();
    let occ = images.add(grid_image(map, |x, y| occluders.boxes[map.idx(x, y)]));
    let extra = images.add(grid_image(map, |x, y| {
        [
            occluders.objects[map.idx(x, y)],
            if map.at(x, y).indoor { 255 } else { 0 },
            if map.at(x, y).wall == Wall::Glass {
                255
            } else {
                0
            },
            0,
        ]
    }));

    let ground = materials.add(WorldMaterial {
        params: params(map, 0.0),
        tiles: tiles.clone(),
        occ: occ.clone(),
        extra: extra.clone(),
        blend: false,
    });
    let things = materials.add(WorldMaterial {
        params: params(map, 1.0),
        tiles,
        occ,
        extra,
        blend: true,
    });
    world_materials.all = vec![ground.clone(), things.clone()];

    let size = vec2(map.w as f32, map.h as f32) * TILE;
    commands.spawn((
        WorldPart,
        Mesh2d(meshes.add(Rectangle::new(size.x, size.y))),
        MeshMaterial2d(ground),
        Transform::from_xyz(size.x * 0.5, size.y * 0.5, 0.0),
    ));
    let layers = paint::paint_props(&map.props);
    for (k, layer) in layers.into_iter().enumerate() {
        if layer.is_empty() {
            continue;
        }
        let z = [1.0, 2.0, 20.0][k];
        commands.spawn((
            WorldPart,
            Mesh2d(meshes.add(layer.build())),
            MeshMaterial2d(things.clone()),
            Transform::from_xyz(0.0, 0.0, z),
        ));
    }

    lamps.all = map
        .lights
        .iter()
        .map(|l| {
            (
                l.pos,
                vec4(l.pos.x, l.pos.y, l.radius, l.intensity),
                vec4(l.color[0], l.color[1], l.color[2], 0.0),
            )
        })
        .collect();
}

/// The material every mesh of the robber shares, made once a map.
pub fn things_material(world_materials: &WorldMaterials) -> Handle<WorldMaterial> {
    world_materials.all[1].clone()
}

/// Where the eyes are and which lamps light the frame, into every material.
pub fn write_params(
    eye: Vec2,
    clock: f32,
    world_materials: &WorldMaterials,
    lamps: &Lamps,
    materials: &mut Assets<WorldMaterial>,
) {
    let mut near: Vec<&(Vec2, Vec4, Vec4)> = lamps.all.iter().collect();
    if near.len() > MAX_LIGHTS {
        near.sort_by(|a, b| {
            a.0.distance_squared(eye)
                .partial_cmp(&b.0.distance_squared(eye))
                .unwrap()
        });
        near.truncate(MAX_LIGHTS);
    }
    for handle in &world_materials.all {
        let Some(mut m) = materials.get_mut(handle) else {
            continue;
        };
        let p = &mut m.params;
        p.eye.x = eye.x;
        p.eye.y = eye.y;
        p.map.w = clock;
        p.misc.y = near.len() as f32;
        for (i, lamp) in near.iter().enumerate() {
            p.lights[2 * i] = lamp.1;
            p.lights[2 * i + 1] = lamp.2;
        }
    }
}
