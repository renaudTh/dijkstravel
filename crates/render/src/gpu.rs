//! Rendu du relief sur le GPU (wgpu, sans fenêtre) : ciel, puis maillages des morceaux d'atlas texturés
//! par ces morceaux, avec un tampon de profondeur ; l'image et la profondeur reviennent en mémoire pour
//! que Skia dessine le tracé, les lieux et la carte d'infos par-dessus.
//!
//! Un seul passage GPU à la fois (les images se rendent en parallèle sur le CPU) : textures et maillages
//! restent en mémoire vidéo d'une image à l'autre.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bytemuck::{Pod, Zeroable};
use skia_safe::{AlphaType, Canvas, ColorType, Image, ImageInfo};

use crate::atlas::{APRON, CHUNK, ChunkLayers};

use crate::cache::Lru;
use crate::compose::Composer;
use crate::mesh::{self, Vertex};

pub(crate) const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// L'image rendue, dans l'ordre des octets du canevas de Skia (N32 : BGRA) : relue, elle s'y recopie
/// ligne à ligne sans conversion.
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Textures de morceaux gardées en mémoire vidéo : 1,4 Mo chacune avec leurs mipmaps, 700 Mo en tout. Une
/// image en 4K en lit deux cents environ, le double en transfert.
const TEXTURES: usize = 512;
/// Maillages gardés en mémoire vidéo : 50 Ko chacun.
const MESHES: usize = 768;
/// Échantillons par pixel de l'image : crêtes lisses contre le ciel. La profondeur relue vient d'une
/// seconde passe à un échantillon, un tampon multi-échantillonné ne se relisant pas.
const SAMPLES: u32 = 4;
/// Mémoire au plus pour les tampons de relecture, et leur nombre au plus : chaque image en cours de
/// relecture a les siens, pendant que le GPU dessine les suivantes.
const READBACK_BUDGET: u64 = 1 << 29;
const MAX_READBACKS: u64 = 8;
/// Côté de la carte des ombres, vue depuis le soleil.
pub const SHADOW_SIZE: u32 = 4096;

/// Niveaux de mipmap au plus : contre le fourmillement des morceaux lointains vus en rasant.
pub(crate) const MIP_LEVELS: u32 = 6;

/// Réglages d'une image, communs à tous les morceaux.
#[derive(Clone, Copy)]
pub struct FrameSettings {
    /// Repère local → écran (`Perspective::view_projection`).
    pub view_projection: [[f32; 4]; 4],
    /// Caméra en repère local.
    pub eye: [f32; 3],
    /// Pixels monde par mètre d'altitude, exagération comprise.
    pub z_per_meter: f32,
    /// Altitude du point visé, en mètres.
    pub ground: f32,
    /// Distance au sol (pixels monde) où le brouillard commence, et où il recouvre tout.
    pub fog: [f32; 2],
    /// RGBA de 0 à 1.
    pub fog_color: [f32; 4],
    pub sky_color: [f32; 4],
    /// Horizon en y normalisé (−1 en bas de l'écran, 1 en haut ; au-delà de 1 : pas de ciel).
    pub horizon: f32,
    /// Part du fond des transferts.
    pub transfer_blend: f32,
    /// Bordure de la texture d'un morceau, et son côté utile, en part de la texture.
    pub uv_inset: f32,
    pub uv_scale: f32,
    /// Écran → repère local, pour la direction de chaque pixel du ciel.
    pub inverse_view_projection: [[f32; 4]; 4],
    /// Exagération du relief : les pentes des maillages en prennent autant.
    pub exaggeration: f32,
    pub light: Light,
    /// Repère local → carte des ombres (vue orthographique depuis le soleil), et côté d'un de ses pixels en
    /// repère local ; aucune ombre portée sans.
    pub shadow: Option<([[f32; 4]; 4], f32)>,
}

/// Lumière du soleil sur le relief (`light::Sun`) ; sans effet à `strength` nulle.
#[derive(Clone, Copy, Default)]
pub struct Light {
    /// Vers le soleil, en repère local, unitaire.
    pub direction: [f32; 3],
    /// De 0 (les couleurs de l'atlas telles quelles) à 1 (éclairées par le soleil et le ciel).
    pub strength: f32,
    /// Lumière directe du soleil et lumière du ciel, RVB.
    pub sun: [f32; 3],
    pub ambient: [f32; 3],
    /// Gain sur le relief éclairé : le soleil couchant reste lisible.
    pub exposure: f32,
}

/// Un morceau d'atlas à dessiner.
pub struct ChunkDraw<'a> {
    /// Identifie la texture du morceau d'une image à l'autre.
    pub key: u64,
    /// Identifie son maillage.
    pub mesh_key: u64,
    /// Identifie la texture du même morceau dans le fond des transferts, s'il se voit.
    pub transfer_key: Option<u64>,
    /// Coin haut gauche du morceau en repère local, côté en pixels monde au zoom de la caméra.
    pub origin: [f32; 2],
    pub size: f32,
    pub mesh: &'a dyn Fn() -> Arc<Vec<Vertex>>,
    /// Les calques du morceau, lus seulement si sa texture n'est pas déjà sur le GPU, qui les compose.
    pub image: &'a dyn Fn() -> ChunkLayers<'a>,
    pub transfer_image: &'a dyn Fn() -> Option<ChunkLayers<'a>>,
}

/// Un morceau qui projette son ombre sans être dessiné : ceux de la carte des ombres couvrent le sol autour
/// du point visé, vu ou non, pour qu'une crête hors champ fasse son ombre.
pub struct CasterDraw<'a> {
    pub mesh_key: u64,
    /// Coin haut gauche en repère local, côté en pixels monde au zoom de la caméra.
    pub origin: [f32; 2],
    pub size: f32,
    pub mesh: &'a dyn Fn() -> Arc<Vec<Vertex>>,
}

/// La profondeur de l'image rendue (0 au plan proche, 1 au lointain ou dans le ciel), ligne par ligne, à
/// `depth_size` : la même vue, en moins de pixels pour une grande image (`depth_size`).
pub struct Rendered {
    pub depth: Vec<f32>,
    pub depth_size: [u32; 2],
}

/// Taille de la profondeur relue d'une image : petit côté de 1080 pixels au plus, la résolution du
/// rendu de référence. Elle ne sert qu'à cacher le tracé derrière le relief ; plus fine, sa relecture
/// coûterait autant que celle de l'image.
pub fn depth_size(width: u32, height: u32) -> [u32; 2] {
    let factor = (width.min(height) / 1080).max(1);
    [width.div_ceil(factor), height.div_ceil(factor)]
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_projection: [[f32; 4]; 4],
    eye: [f32; 4],
    fog_color: [f32; 4],
    sky_color: [f32; 4],
    params: [f32; 4],
    uv: [f32; 4],
    inverse_view_projection: [[f32; 4]; 4],
    sun: [f32; 4],
    sun_color: [f32; 4],
    ambient: [f32; 4],
    light_view_projection: [[f32; 4]; 4],
    /// x : côté d'un pixel de la carte des ombres en repère local ; y : 1 si elle est dessinée.
    shadow: [f32; 4],
    /// Largeur et hauteur de l'image, en pixels.
    screen: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ChunkUniform {
    origin_size: [f32; 4],
}

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    width: u32,
    height: u32,
    sky: wgpu::RenderPipeline,
    terrain: wgpu::RenderPipeline,
    /// Le relief sans couleur, à un échantillon : la profondeur relue.
    terrain_depth: wgpu::RenderPipeline,
    frame_layout: wgpu::BindGroupLayout,
    /// Le relief vu depuis le soleil, en profondeur seule : la carte des ombres.
    shadow: wgpu::RenderPipeline,
    shadow_layout: wgpu::BindGroupLayout,
    /// Compare la profondeur d'un point vu du soleil à celle de la carte des ombres, filtrée.
    shadow_sampler: wgpu::Sampler,
    chunk_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    indices: wgpu::Buffer,
    index_count: u32,
    /// Écart entre deux `ChunkUniform` dans le tampon des morceaux (alignement des décalages dynamiques).
    chunk_stride: u64,
    textures: Lru<u64, Option<Arc<wgpu::TextureView>>>,
    composer: Composer,
    meshes: Lru<u64, Arc<wgpu::Buffer>>,
    /// Surfaces de rendu, prises le temps d'enregistrer et de soumettre une image : le GPU exécute les
    /// soumissions dans l'ordre, la suivante peut les reprendre aussitôt.
    targets: Mutex<Targets>,
    /// Tampons de relecture, chacun pris par une image jusqu'à ce qu'elle soit relue.
    readbacks: Vec<Mutex<Readback>>,
    /// Tampons à essayer en premier, à tour de rôle.
    next_readback: AtomicUsize,
}

/// Les surfaces d'un passage.
struct Targets {
    /// L'image multi-échantillonnée, résolue dans `color`.
    samples: wgpu::Texture,
    samples_depth: wgpu::Texture,
    color: wgpu::Texture,
    depth: wgpu::Texture,
    shadow: wgpu::Texture,
}

/// L'image et sa profondeur copiées pour être relues.
struct Readback {
    color: wgpu::Buffer,
    depth: wgpu::Buffer,
}

impl Gpu {
    /// Le GPU de la machine, ou à défaut le rendu logiciel (lent) ; une erreur sans l'un ni l'autre.
    pub fn new(width: u32, height: u32) -> Result<Gpu, String> {
        pollster::block_on(Self::open(width, height))
    }

    async fn open(width: u32, height: u32) -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await
        {
            Ok(adapter) => adapter,
            Err(_) => {
                let adapter = instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        force_fallback_adapter: true,
                        ..Default::default()
                    })
                    .await
                    .map_err(|error| format!("aucun GPU, même logiciel, pour le relief : {error}"))?;
                eprintln!("relief : pas de GPU, rendu logiciel ({}), lent", adapter.get_info().name);
                adapter
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("relief"),
                required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("GPU du relief : {error}"))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("relief"),
            source: wgpu::ShaderSource::Wgsl(include_str!("terrain.wgsl").into()),
        });
        let uniform = |binding: u32, visibility: wgpu::ShaderStages, dynamic: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: None,
            },
            count: None,
        };
        let texture = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image"),
            entries: &[
                uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ombres"),
            entries: &[uniform(0, wgpu::ShaderStages::VERTEX, false)],
        });
        let chunk_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("morceau"),
            entries: &[uniform(0, wgpu::ShaderStages::VERTEX, true)],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("textures"),
            entries: &[texture(0), texture(1)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("relief"),
            bind_group_layouts: &[Some(&frame_layout), Some(&chunk_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let color_target =
            [Some(wgpu::ColorTargetState { format: TARGET_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })];
        let pipeline = |vertex: &str,
                        fragment: Option<&str>,
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        depth_write: bool,
                        samples: u32| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(vertex),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(if depth_write {
                        wgpu::CompareFunction::Less
                    } else {
                        wgpu::CompareFunction::Always
                    }),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
                fragment: fragment.map(|fragment| wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &color_target,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2],
        };
        let sky = pipeline("sky_vertex", Some("sky_fragment"), &[], false, SAMPLES);
        let terrain =
            pipeline("terrain_vertex", Some("terrain_fragment"), &[Some(vertex_layout.clone())], true, SAMPLES);
        let terrain_depth = pipeline("terrain_vertex", None, &[Some(vertex_layout.clone())], true, 1);
        let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ombres"),
            bind_group_layouts: &[Some(&shadow_layout), Some(&chunk_layout)],
            immediate_size: 0,
        });
        let shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ombres"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("shadow_vertex"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout)],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                // Contre l'acné : un versant ne s'ombre pas lui-même.
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ombres"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("morceaux"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });
        let index_data = mesh::indices();
        let indices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("triangles"),
            size: size_of_val(index_data) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&indices, 0, bytemuck::cast_slice(index_data));
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let chunk_stride = (size_of::<ChunkUniform>() as u64).div_ceil(alignment) * alignment;
        let count = (READBACK_BUDGET / Readback::bytes(width, height)).clamp(1, MAX_READBACKS);
        let readbacks = (0..count).map(|_| Mutex::new(Readback::new(&device, width, height))).collect();
        let targets = Mutex::new(Targets::new(&device, width, height));
        Ok(Gpu {
            width,
            height,
            sky,
            terrain,
            terrain_depth,
            frame_layout,
            shadow,
            shadow_layout,
            shadow_sampler,
            chunk_layout,
            texture_layout,
            sampler,
            index_count: index_data.len() as u32,
            indices,
            chunk_stride,
            textures: Lru::new(TEXTURES),
            composer: Composer::new(&device, &queue),
            meshes: Lru::new(MESHES),
            targets,
            readbacks,
            next_readback: AtomicUsize::new(0),
            device,
            queue,
        })
    }

    /// Ciel puis morceaux, relus en mémoire.
    /// L'image se dessine dans `canvas`, qu'elle recouvre ; la profondeur est rendue.
    /// `casters` : les morceaux de la carte des ombres, s'il y en a une.
    pub fn render(
        &self,
        settings: &FrameSettings,
        chunks: &[ChunkDraw],
        casters: &[CasterDraw],
        canvas: &Canvas,
    ) -> Rendered {
        let device = &self.device;
        let light = settings.light;
        let frame = FrameUniform {
            view_projection: settings.view_projection,
            eye: [settings.eye[0], settings.eye[1], settings.eye[2], settings.z_per_meter],
            fog_color: settings.fog_color,
            sky_color: settings.sky_color,
            params: [settings.fog[0], settings.fog[1], settings.horizon, settings.transfer_blend],
            uv: [settings.uv_inset, settings.uv_scale, settings.ground, 0.0],
            inverse_view_projection: settings.inverse_view_projection,
            sun: [light.direction[0], light.direction[1], light.direction[2], light.strength],
            sun_color: [light.sun[0], light.sun[1], light.sun[2], settings.exaggeration],
            ambient: [light.ambient[0], light.ambient[1], light.ambient[2], light.exposure],
            light_view_projection: settings.shadow.map_or([[0.0; 4]; 4], |(matrix, _)| matrix),
            shadow: [settings.shadow.map_or(0.0, |(_, texel)| texel), f32::from(settings.shadow.is_some()), 0.0, 0.0],
            screen: [self.width as f32, self.height as f32, 0.0, 0.0],
        };
        let frame_buffer = self.uniform_buffer("image", bytemuck::bytes_of(&frame));
        let shadow_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ombres"),
            layout: &self.shadow_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame_buffer.as_entire_binding() }],
        });
        let stride = self.chunk_stride as usize;
        // Les morceaux dessinés, puis ceux de la carte des ombres.
        let placements: Vec<([f32; 2], f32)> = chunks
            .iter()
            .map(|chunk| (chunk.origin, chunk.size))
            .chain(casters.iter().map(|caster| (caster.origin, caster.size)))
            .collect();
        let mut chunk_data = vec![0u8; stride * placements.len().max(1)];
        for (i, (origin, size)) in placements.iter().enumerate() {
            let uniform = ChunkUniform { origin_size: [origin[0], origin[1], *size, 0.0] };
            chunk_data[i * stride..i * stride + size_of::<ChunkUniform>()]
                .copy_from_slice(bytemuck::bytes_of(&uniform));
        }
        let chunk_buffer = self.uniform_buffer("morceaux", &chunk_data);
        let chunk_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("morceau"),
            layout: &self.chunk_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &chunk_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(size_of::<ChunkUniform>() as u64),
                }),
            }],
        });
        let draws: Vec<(wgpu::BindGroup, Arc<wgpu::Buffer>)> = chunks
            .iter()
            .map(|chunk| {
                let main = self.texture(chunk.key, chunk.image);
                let transfer = chunk
                    .transfer_key
                    .and_then(|key| {
                        self.textures.get_or_insert_with(&key, || {
                            (chunk.transfer_image)().map(|layers| Arc::new(self.compose(&layers)))
                        })
                    })
                    .unwrap_or_else(|| main.clone());
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("textures"),
                    layout: &self.texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&main) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&transfer) },
                    ],
                });
                (group, self.mesh(chunk.mesh_key, chunk.mesh))
            })
            .collect();

        let readback = self.free_readback();
        let targets = self.targets.lock().unwrap();
        let color_view = targets.color.create_view(&Default::default());
        let depth_view = targets.depth.create_view(&Default::default());
        let samples_view = targets.samples.create_view(&Default::default());
        let samples_depth_view = targets.samples_depth.create_view(&Default::default());
        let shadow_view = targets.shadow.create_view(&Default::default());
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image"),
            layout: &self.frame_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: frame_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&shadow_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.shadow_sampler) },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("relief") });
        if settings.shadow.is_some() {
            // Le relief vu depuis le soleil : ce qu'il voit est au soleil, le reste à l'ombre.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ombres"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shadow);
            pass.set_bind_group(0, &shadow_group, &[]);
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            for (i, caster) in casters.iter().enumerate() {
                let mesh = self.mesh(caster.mesh_key, caster.mesh);
                pass.set_bind_group(1, &chunk_group, &[((chunks.len() + i) * stride) as u32]);
                pass.set_vertex_buffer(0, mesh.slice(..));
                pass.draw_indexed(0..self.index_count, 0, 0..1);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("relief"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &samples_view,
                    depth_slice: None,
                    resolve_target: Some(&color_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &samples_depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &frame_group, &[]);
            pass.set_bind_group(1, &chunk_group, &[0]);
            pass.set_bind_group(
                2,
                &draws.first().map(|(group, _)| group.clone()).unwrap_or_else(|| self.empty_group()),
                &[],
            );
            pass.set_pipeline(&self.sky);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(&self.terrain);
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            for (i, (group, mesh)) in draws.iter().enumerate() {
                pass.set_bind_group(1, &chunk_group, &[(i * stride) as u32]);
                pass.set_bind_group(2, group, &[]);
                pass.set_vertex_buffer(0, mesh.slice(..));
                pass.draw_indexed(0..self.index_count, 0, 0..1);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("profondeur"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.terrain_depth);
            pass.set_bind_group(0, &frame_group, &[]);
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            for (i, (group, mesh)) in draws.iter().enumerate() {
                pass.set_bind_group(1, &chunk_group, &[(i * stride) as u32]);
                pass.set_bind_group(2, group, &[]);
                pass.set_vertex_buffer(0, mesh.slice(..));
                pass.draw_indexed(0..self.index_count, 0, 0..1);
            }
        }
        let (width, height) = (self.width, self.height);
        let [depth_width, depth_height] = depth_size(width, height);
        let row = padded_row(width);
        for (texture, buffer, aspect, [w, h]) in [
            (&targets.color, &readback.color, wgpu::TextureAspect::All, [width, height]),
            (&targets.depth, &readback.depth, wgpu::TextureAspect::DepthOnly, [depth_width, depth_height]),
        ] {
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect },
                wgpu::TexelCopyBufferInfo {
                    buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_row(w)),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        }
        let submission = self.queue.submit([encoder.finish()]);
        drop(targets);
        for buffer in [&readback.color, &readback.depth] {
            buffer.map_async(wgpu::MapMode::Read, .., |result| result.expect("relecture du relief"));
        }
        // Cette image seulement : les autres threads soumettent les leurs pendant ce temps.
        device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: None }).expect("GPU du relief");
        let row = row as usize;
        // Une seule copie par tampon : l'image droit dans le canevas, la profondeur en flottants.
        let mapped = readback.color.get_mapped_range(..).expect("relecture du relief");
        paint(canvas, &mapped[..row * height as usize], row, width, height);
        drop(mapped);
        readback.color.unmap();
        let mapped = readback.depth.get_mapped_range(..).expect("relecture du relief");
        let mut depth: Vec<f32> = Vec::with_capacity(depth_width as usize * depth_height as usize);
        for line in mapped.chunks(padded_row(depth_width) as usize).take(depth_height as usize) {
            depth.extend_from_slice(bytemuck::cast_slice(&line[..depth_width as usize * 4]));
        }
        drop(mapped);
        readback.depth.unmap();
        drop(readback);
        Rendered { depth, depth_size: [depth_width, depth_height] }
    }

    /// Des tampons de relecture libres, sinon les suivants à tour de rôle dès qu'ils se libèrent.
    fn free_readback(&self) -> std::sync::MutexGuard<'_, Readback> {
        let count = self.readbacks.len();
        let start = self.next_readback.fetch_add(1, Ordering::Relaxed);
        (0..count)
            .find_map(|i| self.readbacks[(start + i) % count].try_lock().ok())
            .unwrap_or_else(|| self.readbacks[start % count].lock().unwrap())
    }

    fn uniform_buffer(&self, label: &str, data: &[u8]) -> wgpu::Buffer {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: data.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&buffer, 0, data);
        buffer
    }

    /// Le ciel lit le groupe des textures sans s'en servir : un groupe quelconque, s'il n'y a aucun morceau.
    fn empty_group(&self) -> wgpu::BindGroup {
        let blank =
            Arc::new(upload(&self.device, &self.queue, &RgbaImage { width: 1, height: 1, pixels: vec![0; 4] }, 1));
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("textures"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&blank) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&blank) },
            ],
        })
    }

    fn texture<'a>(&self, key: u64, layers: &dyn Fn() -> ChunkLayers<'a>) -> Arc<wgpu::TextureView> {
        self.textures.get_or_insert_with(&key, || Some(Arc::new(self.compose(&layers())))).expect("texture du morceau")
    }

    fn compose(&self, layers: &ChunkLayers) -> wgpu::TextureView {
        self.composer.compose(layers, (CHUNK + 2 * APRON) as u32)
    }

    fn mesh(&self, key: u64, mesh: &dyn Fn() -> Arc<Vec<Vertex>>) -> Arc<wgpu::Buffer> {
        self.meshes.get_or_insert_with(&key, || {
            let vertices = mesh();
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("maillage"),
                size: size_of_val(vertices.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buffer, 0, bytemuck::cast_slice(vertices.as_slice()));
            Arc::new(buffer)
        })
    }
}

/// Texture et ses mipmaps (`max_levels` niveaux au plus), réduites deux par deux sur le CPU.
pub(crate) fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: &RgbaImage,
    max_levels: u32,
) -> wgpu::TextureView {
    let levels = max_levels.min(32 - image.width.max(image.height).leading_zeros());
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("morceau"),
        size: wgpu::Extent3d { width: image.width, height: image.height, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut level = image.clone();
    for mip_level in 0..levels {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &level.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(level.width * 4),
                rows_per_image: Some(level.height),
            },
            wgpu::Extent3d { width: level.width, height: level.height, depth_or_array_layers: 1 },
        );
        level = level.half();
    }
    texture.create_view(&Default::default())
}

impl Readback {
    /// Octets d'un tampon de `width × height` pixels de 4 octets, lignes alignées.
    fn size(width: u32, height: u32) -> u64 {
        u64::from(padded_row(width)) * u64::from(height)
    }

    /// Mémoire des deux tampons.
    fn bytes(width: u32, height: u32) -> u64 {
        let [depth_width, depth_height] = depth_size(width, height);
        Self::size(width, height) + Self::size(depth_width, depth_height)
    }

    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let buffer = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let [depth_width, depth_height] = depth_size(width, height);
        Readback {
            color: buffer("image relue", Self::size(width, height)),
            depth: buffer("profondeur relue", Self::size(depth_width, depth_height)),
        }
    }
}

impl Targets {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let sized = |[width, height]: [u32; 2], label: &str, format: wgpu::TextureFormat, sample_count: u32| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let target = |label: &str, format, sample_count| sized([width, height], label, format, sample_count);
        Targets {
            samples: target("échantillons", TARGET_FORMAT, SAMPLES),
            samples_depth: target("profondeur des échantillons", DEPTH_FORMAT, SAMPLES),
            color: target("image", TARGET_FORMAT, 1),
            // La passe de profondeur se rend à sa propre taille : même vue, moins de pixels.
            depth: sized(depth_size(width, height), "profondeur", DEPTH_FORMAT, 1),
            shadow: device.create_texture(&wgpu::TextureDescriptor {
                label: Some("ombres"),
                size: wgpu::Extent3d { width: SHADOW_SIZE, height: SHADOW_SIZE, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }),
        }
    }
}

/// Pose l'image relue (BGRA, lignes de `row` octets) dans `canvas` : recopiée ligne à ligne dans ses
/// pixels quand il en a de même format, sinon dessinée.
fn paint(canvas: &Canvas, bytes: &[u8], row: usize, width: u32, height: u32) {
    let tight = width as usize * 4;
    if let Some(target) = canvas.access_top_layer_pixels()
        && target.info.color_type() == ColorType::BGRA8888
        && target.info.width() == width as i32
        && target.info.height() == height as i32
    {
        for (line, out) in bytes.chunks(row).zip(target.pixels.chunks_mut(target.row_bytes)) {
            out[..tight].copy_from_slice(&line[..tight]);
        }
        return;
    }
    let info = ImageInfo::new((width as i32, height as i32), ColorType::BGRA8888, AlphaType::Premul, None);
    if let Some(image) = skia_safe::images::raster_from_data(&info, skia_safe::Data::new_copy(bytes), row) {
        canvas.draw_image(&image, (0, 0), None);
    }
}

/// Octets par ligne d'une copie vers un tampon : 4 par pixel, alignés comme le veut wgpu.
fn padded_row(width: u32) -> u32 {
    (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
}

/// Pixels RGBA prémultipliés, ligne par ligne.
#[derive(Clone)]
pub(crate) struct RgbaImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

impl RgbaImage {
    pub(crate) fn of(image: &Image) -> Self {
        let (width, height) = (image.width() as u32, image.height() as u32);
        let info = ImageInfo::new((width as i32, height as i32), ColorType::RGBA8888, AlphaType::Premul, None);
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        image.read_pixels(&info, &mut pixels, width as usize * 4, (0, 0), skia_safe::image::CachingHint::Disallow);
        RgbaImage { width, height, pixels }
    }

    /// Moitié de la taille (arrondie en dessous, au moins 1), moyenne de chaque carré de 2 × 2 pixels.
    fn half(&self) -> Self {
        let (width, height) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        let at = |x: u32, y: u32, c: u32| {
            let (x, y) = (x.min(self.width - 1), y.min(self.height - 1));
            u32::from(self.pixels[((y * self.width + x) * 4 + c) as usize])
        };
        for y in 0..height {
            for x in 0..width {
                for c in 0..4 {
                    let sum = at(2 * x, 2 * y, c)
                        + at(2 * x + 1, 2 * y, c)
                        + at(2 * x, 2 * y + 1, c)
                        + at(2 * x + 1, 2 * y + 1, c);
                    pixels.push(((sum + 2) / 4) as u8);
                }
            }
        }
        RgbaImage { width, height, pixels }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Camera;
    use crate::mesh::GRID;
    use crate::perspective::Perspective;
    use skia_safe::{Color, surfaces};

    /// Un morceau plat d'une couleur, sous une caméra inclinée : le centre est de sa couleur, le haut de
    /// l'écran est du ciel, plus loin que le sol.
    #[test]
    fn draws_the_sky_and_a_textured_chunk_and_reads_back_the_depth() {
        let (width, height) = (270, 480);
        let Ok(gpu) = Gpu::new(width, height) else {
            eprintln!("pas de GPU : test sauté");
            return;
        };
        let camera = Camera { pitch: 70.0, bearing: 0.0, ..Camera::flat([6.0, 45.0], 12.0) };
        let perspective = Perspective::new(camera, width, height, 0.0, 1.0);
        let flat: Arc<Vec<Vertex>> = Arc::new(
            (0..=GRID)
                .flat_map(|j| (0..=GRID).map(move |i| [i as f32 / GRID as f32, j as f32 / GRID as f32, 0.0, 0.0, 0.0]))
                .chain((0..4 * GRID).map(|_| [0.0, 0.0, -10.0, 0.0, 0.0]))
                .collect(),
        );
        let mesh = || flat.clone();
        // Un morceau rouge uni : le fond seul, composé par le GPU.
        let image = || ChunkLayers {
            background: [1.0, 0.0, 0.0, 1.0],
            adjust: [0.0; 20],
            tiles: Vec::new(),
            hillshade: None,
            tint: [0.0; 4],
            borders: None,
        };
        let none = || None;
        let to_f32 = |m: glam::DMat4| m.to_cols_array_2d().map(|c| c.map(|v| v as f32));
        let settings = FrameSettings {
            view_projection: to_f32(perspective.view_projection()),
            eye: [0.0, 0.0, 0.0],
            z_per_meter: 0.0,
            ground: 0.0,
            fog: [1e9, 2e9],
            fog_color: [1.0, 1.0, 1.0, 1.0],
            sky_color: [0.0, 0.0, 1.0, 1.0],
            horizon: 0.5,
            transfer_blend: 0.0,
            uv_inset: 0.0,
            uv_scale: 1.0,
            inverse_view_projection: to_f32(perspective.view_projection().inverse()),
            exaggeration: 1.0,
            light: Light::default(),
            shadow: None,
        };
        // Un grand morceau centré sur le point visé.
        let size = 4000.0;
        let chunk = ChunkDraw {
            key: 1,
            mesh_key: 1,
            transfer_key: None,
            origin: [-size / 2.0, size / 2.0],
            size,
            mesh: &mesh,
            image: &image,
            transfer_image: &none,
        };
        let mut surface = surfaces::raster_n32_premul((width as i32, height as i32)).unwrap();
        let rendered = gpu.render(&settings, &[chunk], &[], surface.canvas());
        let image = surface.image_snapshot();
        let pixel = |x: i32, y: i32| image.peek_pixels().unwrap().get_color((x, y));
        assert_eq!(pixel(135, 240), Color::RED);
        assert_eq!(pixel(135, 0), Color::BLUE);
        let depth = |x: usize, y: usize| rendered.depth[y * rendered.depth_size[0] as usize + x];
        assert!(depth(135, 240) < 1.0 && depth(135, 0) == 1.0);
        // Même profondeur que celle que calcule `Perspective` pour le point visé.
        let (_, expected) = perspective.screen_at([6.0, 45.0], 0.0).unwrap();
        assert!((f64::from(depth(135, 240)) - expected).abs() < 1e-4, "{} ≠ {expected}", depth(135, 240));
    }
}
