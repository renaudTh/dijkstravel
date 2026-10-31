//! Morceaux d'atlas de la vue inclinée composés sur le GPU, calque par calque comme `Atlas::render_chunk` :
//! fond, tuiles d'imagerie retouchées, ombrage agrandi, voile, frontières, puis les mipmaps réduites deux
//! par deux. Le CPU ne fait plus que décoder les tuiles, gardées en mémoire vidéo d'un morceau à l'autre.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use skia_safe::Image;

use crate::atlas::ChunkLayers;
use crate::cache::Lru;
use crate::gpu::{COLOR_FORMAT, MIP_LEVELS, RgbaImage, upload};

/// Tuiles d'imagerie gardées en mémoire vidéo : 350 Ko chacune avec leurs mipmaps.
const TILES: usize = 768;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerUniform {
    rect: [f32; 4],
    target: [f32; 4],
    matrix: [[f32; 4]; 4],
    offset: [f32; 4],
    color: [f32; 4],
}

/// Comment `layer_fragment` colore un calque.
#[derive(Clone, Copy)]
enum Mode {
    Texture = 0,
    Adjusted = 1,
    Solid = 2,
}

impl LayerUniform {
    fn new(rect: [f32; 4], target: [f32; 2], mode: Mode) -> Self {
        LayerUniform {
            rect,
            target: [target[0], target[1], mode as u32 as f32, 0.0],
            matrix: [[0.0; 4]; 4],
            offset: [0.0; 4],
            color: [0.0; 4],
        }
    }
}

pub struct Composer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layer_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    /// Comme le dessin de l'imagerie par Skia (et MapLibre) : bilinéaire, mipmap la plus proche.
    sampler: wgpu::Sampler,
    /// Écart entre deux `LayerUniform` du tampon des calques (alignement des décalages dynamiques).
    stride: u64,
    /// Texture de remplacement des calques unis, qui n'en lisent pas.
    blank: wgpu::TextureView,
    tiles: Lru<u64, Option<Arc<wgpu::TextureView>>>,
}

impl Composer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composition"),
            source: wgpu::ShaderSource::Wgsl(include_str!("compose.wgsl").into()),
        });
        let layer_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("calque"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("calque texturé"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composition"),
            bind_group_layouts: &[Some(&layer_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        // Par-dessus, en couleurs prémultipliées (`SrcOver` de Skia).
        let over = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composition"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("layer_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("layer_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: Some(wgpu::BlendState { color: over, alpha: over }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("calques"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let blank = upload(device, queue, &RgbaImage { width: 1, height: 1, pixels: vec![0; 4] }, 1);
        Composer {
            device: device.clone(),
            queue: queue.clone(),
            pipeline,
            layer_layout,
            texture_layout,
            sampler,
            stride: (size_of::<LayerUniform>() as u64).div_ceil(alignment) * alignment,
            blank,
            tiles: Lru::new(TILES),
        }
    }

    /// La texture du morceau, `side × side` pixels et ses mipmaps.
    pub fn compose(&self, layers: &ChunkLayers, side: u32) -> wgpu::TextureView {
        let device = &self.device;
        let levels = MIP_LEVELS.min(32 - side.leading_zeros());
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("morceau composé"),
            size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let target = [side as f32; 2];
        let whole = [0.0, 0.0, side as f32, side as f32];
        // Calques du premier niveau, chacun avec sa texture.
        let mut calls: Vec<(LayerUniform, Arc<wgpu::TextureView>)> = Vec::new();
        let m = &layers.adjust;
        let adjusted = |rect| {
            let mut uniform = LayerUniform::new(rect, target, Mode::Adjusted);
            for row in 0..4 {
                for column in 0..4 {
                    uniform.matrix[column][row] = m[row * 5 + column];
                }
                uniform.offset[row] = m[row * 5 + 4];
            }
            uniform
        };
        for tile in &layers.tiles {
            let view = self.tiles.get_or_insert_with(&tile.key, || {
                (tile.image)().map(|image| Arc::new(upload(device, &self.queue, &RgbaImage::of(&image), MIP_LEVELS)))
            });
            if let Some(view) = view {
                calls.push((adjusted(tile.rect), view));
            }
        }
        let texture_of = |image: &Image| Arc::new(upload(device, &self.queue, &RgbaImage::of(image), 1));
        if let Some((grid, [width, height])) = &layers.hillshade {
            calls.push((LayerUniform::new([0.0, 0.0, *width, *height], target, Mode::Texture), texture_of(grid)));
        }
        let blank = Arc::new(self.blank.clone());
        if layers.tint[3] > 0.0 {
            let mut uniform = LayerUniform::new(whole, target, Mode::Solid);
            uniform.color = layers.tint;
            calls.push((uniform, blank.clone()));
        }
        if let Some(borders) = &layers.borders {
            calls.push((LayerUniform::new(whole, target, Mode::Texture), texture_of(borders)));
        }
        // Puis chaque niveau de mipmap, réduction bilinéaire du précédent.
        let level_view = |level: u32| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let level_side = |level: u32| (side >> level).max(1) as f32;
        let level_views: Vec<wgpu::TextureView> = (0..levels).map(level_view).collect();
        let reductions = (1..levels).map(|level| {
            let side = level_side(level);
            LayerUniform::new([0.0, 0.0, side, side], [side; 2], Mode::Texture)
        });
        let uniforms: Vec<LayerUniform> = calls.iter().map(|(uniform, _)| *uniform).chain(reductions).collect();
        let stride = self.stride as usize;
        let mut data = vec![0u8; stride * uniforms.len().max(1)];
        for (i, uniform) in uniforms.iter().enumerate() {
            data[i * stride..i * stride + size_of::<LayerUniform>()].copy_from_slice(bytemuck::bytes_of(uniform));
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("calques"),
            size: data.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&buffer, 0, &data);
        let layer_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("calque"),
            layout: &self.layer_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(size_of::<LayerUniform>() as u64),
                    }),
                },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let texture_group = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("calque texturé"),
                layout: &self.texture_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }],
            })
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("composition") });
        let mut index = 0;
        for level in 0..levels {
            let clear = if level == 0 {
                let [r, g, b, a] = layers.background.map(f64::from);
                wgpu::Color { r, g, b, a }
            } else {
                wgpu::Color::TRANSPARENT
            };
            let groups: Vec<wgpu::BindGroup> = if level == 0 {
                calls.iter().map(|(_, view)| texture_group(view)).collect()
            } else {
                vec![texture_group(&level_views[level as usize - 1])]
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composition"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &level_views[level as usize],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            for group in &groups {
                pass.set_bind_group(0, &layer_group, &[(index * stride) as u32]);
                pass.set_bind_group(1, group, &[]);
                pass.draw(0..4, 0..1);
                index += 1;
            }
        }
        self.queue.submit([encoder.finish()]);
        texture.create_view(&Default::default())
    }
}
