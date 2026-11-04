//! Module icons in the overlay: each is rasterised once per physical size and kept as a
//! texture while it stays on screen.
use super::{Growable, storage_entry, uniform_entry};
use crate::scene::UiImage;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct IconGpu {
    rect: [f32; 4],
    tint: [f32; 4],
    params: [f32; 4],
}

struct Raster {
    view: wgpu::TextureView,
    used: bool,
}

pub(super) struct IconSystem {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    instances: Growable,
    rasters: HashMap<(u64, u32, u32), Raster>,
    /// Per overlay layer: the instance index and bind group of each icon it draws.
    draws: Vec<Vec<(u32, wgpu::BindGroup)>>,
}

impl IconSystem {
    pub(super) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let vis = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("icons"),
            entries: &[
                uniform_entry(0, vis),
                storage_entry(1, vis),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("icon.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/icon.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("icons"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_icon"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_icon"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            layout,
            pipeline,
            instances: Growable::new(device, "icons", wgpu::BufferUsages::STORAGE, 1024),
            rasters: HashMap::new(),
            draws: Vec::new(),
        }
    }

    /// Rasterises what is new and uploads this frame's icons, layer by layer.
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &wgpu::Buffer,
        scale: f32,
        layers: &[&[UiImage]],
    ) {
        for r in self.rasters.values_mut() {
            r.used = false;
        }
        let mut data = Vec::new();
        let mut keys: Vec<Vec<(u64, u32, u32)>> = Vec::new();
        for images in layers {
            let mut layer = Vec::new();
            for image in *images {
                let w = (image.rect.w * scale).round().clamp(1.0, 1024.0) as u32;
                let h = (image.rect.h * scale).round().clamp(1.0, 1024.0) as u32;
                let key = (image.icon.hash, w, h);
                self.rasters
                    .entry(key)
                    .or_insert_with(|| Raster {
                        view: upload(device, queue, &image.icon.rasterise(w, h), w, h),
                        used: true,
                    })
                    .used = true;
                layer.push(key);
                data.push(IconGpu {
                    rect: [
                        (image.rect.x * scale).round(),
                        (image.rect.y * scale).round(),
                        w as f32,
                        h as f32,
                    ],
                    tint: image.tint.map_or([1.0; 4], |c| c.to_array()),
                    params: [
                        if image.tint.is_some() { 1.0 } else { 0.0 },
                        image.opacity.clamp(0.0, 1.0),
                        0.0,
                        0.0,
                    ],
                });
            }
            keys.push(layer);
        }
        self.rasters.retain(|_, r| r.used);
        self.instances
            .write(device, queue, bytemuck::cast_slice(&data));
        // Bind groups follow the instance buffer, which may just have grown.
        let mut index = 0u32;
        self.draws = keys
            .iter()
            .map(|layer| {
                layer
                    .iter()
                    .map(|key| {
                        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("icon"),
                            layout: &self.layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: screen.as_entire_binding(),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: self.instances.buf.as_entire_binding(),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 2,
                                    resource: wgpu::BindingResource::TextureView(
                                        &self.rasters[key].view,
                                    ),
                                },
                            ],
                        });
                        index += 1;
                        (index - 1, group)
                    })
                    .collect()
            })
            .collect();
    }

    pub(super) fn render(&self, layer: usize, pass: &mut wgpu::RenderPass<'_>) {
        let Some(draws) = self.draws.get(layer) else {
            return;
        };
        if draws.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        for (i, group) in draws {
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..6, *i..*i + 1);
        }
    }
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pixels: &[u8],
    w: u32,
    h: u32,
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("icon"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * w),
            rows_per_image: Some(h),
        },
        size,
    );
    texture.create_view(&Default::default())
}
