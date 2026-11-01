//! The wgpu renderer: a window or an offscreen target, and one `Scene` per frame.

mod fx;
#[cfg(test)]
mod tests;
mod text;

use crate::backdrop::Backdrop;
use crate::camera::Projection;
use crate::defaults;
use crate::scene::{BackdropHandle, MAX_POINT_LIGHTS, MeshHandle, Panel, PanelShape, Scene, Text};
use crate::style::Ink;
use bytemuck::{Pod, Zeroable};
use fx::FxVertex;
use galaxy_mesh::{MeshData, Vertex};
use glam::{Mat3, Mat4, Vec2};
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use text::TextSystem;
use wgpu::util::DeviceExt;
use winit::window::Window;

/// Multisampling for smooth ink edges, when the adapter supports it.
const MSAA: u32 = 4;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The inner-ink normal and depth buffer. Half floats keep the octahedral normal to
/// a few hundredths of a degree (8-bit unorm wobbled by about a degree, enough to
/// flicker a crease near the art bible's angle as a ship turned). Core WebGPU, so
/// every backend renders to it.
const GEOM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Backdrop scroll, in logical pixels per world unit moved by the camera.
const STAR_PARALLAX: f32 = 0.6;
const CLOUD_PARALLAX: f32 = 0.25;

/// Something went wrong creating or using the GPU.
#[derive(Debug)]
pub enum RenderError {
    /// No suitable GPU adapter (or no GPU at all).
    NoAdapter(String),
    Device(String),
    Surface(String),
    Text(String),
    Readback(String),
    Png(String),
    Io(std::io::Error),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter(e) => write!(f, "no GPU adapter: {e}"),
            Self::Device(e) => write!(f, "GPU device: {e}"),
            Self::Surface(e) => write!(f, "window surface: {e}"),
            Self::Text(e) => write!(f, "text: {e}"),
            Self::Readback(e) => write!(f, "reading the frame back: {e}"),
            Self::Png(e) => write!(f, "PNG: {e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<std::io::Error> for RenderError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// An 8-bit sRGB RGBA image, row-major from the top-left.
#[derive(Clone, Debug, PartialEq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    /// Encodes the image as a PNG file in memory.
    pub fn encode_png(&self) -> Result<Vec<u8>, RenderError> {
        self.encode_png_with(png::Compression::High)
    }

    fn encode_png_with(&self, compression: png::Compression) -> Result<Vec<u8>, RenderError> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
            enc.set_compression(compression);
            let mut w = enc
                .write_header()
                .map_err(|e| RenderError::Png(e.to_string()))?;
            w.write_image_data(&self.pixels)
                .map_err(|e| RenderError::Png(e.to_string()))?;
        }
        Ok(out)
    }

    /// Reads an 8-bit RGBA or RGB PNG.
    pub fn load_png(path: impl AsRef<Path>) -> Result<RgbaImage, RenderError> {
        let file = std::fs::File::open(path)?;
        let decoder = png::Decoder::new(std::io::BufReader::new(file));
        let mut reader = decoder
            .read_info()
            .map_err(|e| RenderError::Png(e.to_string()))?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| RenderError::Png("the image is too large".into()))?;
        let mut buf = vec![0; size];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| RenderError::Png(e.to_string()))?;
        let data = &buf[..info.buffer_size()];
        let pixels = match (info.color_type, info.bit_depth) {
            (png::ColorType::Rgba, png::BitDepth::Eight) => data.to_vec(),
            (png::ColorType::Rgb, png::BitDepth::Eight) => data
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            other => {
                return Err(RenderError::Png(format!(
                    "unsupported PNG layout {other:?}"
                )));
            }
        };
        Ok(RgbaImage {
            width: info.width,
            height: info.height,
            pixels,
        })
    }

    /// How far this image is from `other` of the same size: the mean difference per
    /// channel (0 to 255), and the share of pixels where any channel differs by more
    /// than 16. `None` when the sizes differ.
    pub fn difference(&self, other: &RgbaImage) -> Option<(f64, f64)> {
        if (self.width, self.height) != (other.width, other.height) {
            return None;
        }
        let mut total = 0u64;
        let mut differing = 0u64;
        for (a, b) in self
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(other.pixels.as_chunks::<4>().0)
        {
            let d: Vec<u64> = (0..3).map(|i| u64::from(a[i].abs_diff(b[i]))).collect();
            total += d.iter().sum::<u64>();
            if d.iter().any(|&c| c > 16) {
                differing += 1;
            }
        }
        let n = u64::from(self.width) * u64::from(self.height);
        Some((
            total as f64 / (n.max(1) * 3) as f64,
            differing as f64 / n.max(1) as f64,
        ))
    }

    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<(), RenderError> {
        std::fs::write(path, self.encode_png()?)?;
        Ok(())
    }

    /// Writes a PNG quickly, larger on disk: for the many frames of a film. Still
    /// lossless.
    pub fn save_png_fast(&self, path: impl AsRef<Path>) -> Result<(), RenderError> {
        std::fs::write(path, self.encode_png_with(png::Compression::Fast)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod image_tests {
    use super::RgbaImage;

    #[test]
    fn a_png_round_trips_and_differences_are_measured() {
        let a = RgbaImage {
            width: 2,
            height: 1,
            pixels: vec![10, 20, 30, 255, 200, 100, 0, 255],
        };
        let tmp = std::env::temp_dir().join(format!("galaxy-image-{}.png", std::process::id()));
        a.save_png(&tmp).unwrap();
        let back = RgbaImage::load_png(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(back, a);
        assert_eq!(a.difference(&back), Some((0.0, 0.0)));
        let mut b = a.clone();
        b.pixels[4] = 230;
        let (mean, share) = a.difference(&b).unwrap();
        assert!((mean - 5.0).abs() < 1e-9, "{mean}");
        assert_eq!(share, 0.5);
        let small = RgbaImage {
            width: 1,
            height: 1,
            pixels: vec![0; 4],
        };
        assert_eq!(a.difference(&small), None);
    }
}

// --- GPU data layouts (must match the WGSL structs) ---

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlobalsGpu {
    view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    camera_fwd: [f32; 4],
    viewport: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    ambient: [f32; 4],
    rim: [f32; 4],
    counts: [f32; 4],
    points: [[f32; 4]; MAX_POINT_LIGHTS * 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InstanceGpu {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    tint: [f32; 4],
    shadow: [f32; 4],
    highlight: [f32; 4],
    outline: [f32; 4],
    emissive: [f32; 4],
    /// x: morph weight, y: hatch scale (0 for the instance's own).
    morph: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InkGpu {
    params: [f32; 4],
    color: [f32; 4],
    raster: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SkyGpu {
    top: [f32; 4],
    bottom: [f32; 4],
    clouds: [f32; 4],
    stars: [f32; 4],
    counts: [f32; 4],
    viewport: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlobGpu {
    pos: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PanelGpu {
    rect: [f32; 4],
    params: [f32; 4],
    fill: [f32; 4],
    border: [f32; 4],
    shape: [f32; 4],
}

/// A buffer that grows (by recreation) to fit what is written into it.
struct Growable {
    buf: wgpu::Buffer,
    cap: u64,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl Growable {
    fn new(
        device: &wgpu::Device,
        label: &'static str,
        usage: wgpu::BufferUsages,
        cap: u64,
    ) -> Self {
        let usage = usage | wgpu::BufferUsages::COPY_DST;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: cap,
            usage,
            mapped_at_creation: false,
        });
        Self {
            buf,
            cap,
            usage,
            label,
        }
    }

    /// Writes `data` at the start, growing first if needed. Returns true if recreated.
    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[u8]) -> bool {
        let need = (data.len() as u64).max(16);
        let grew = need > self.cap;
        if grew {
            *self = Self::new(device, self.label, self.usage, need.next_power_of_two());
        }
        if !data.is_empty() {
            queue.write_buffer(&self.buf, 0, data);
        }
        grew
    }
}

/// A mesh slot. The generation bumps whenever the slot is freed, so a stale handle
/// (kept after `remove_mesh`) never aliases a mesh uploaded later into the same slot.
struct MeshSlot {
    generation: u32,
    mesh: Option<GpuMesh>,
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

struct GpuBackdrop {
    source: Backdrop,
    sky: wgpu::Buffer,
    _blobs: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

enum Target {
    Window {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        window: Arc<Window>,
    },
    Headless,
}

struct Pipelines {
    mesh: wgpu::RenderPipeline,
    outline: wgpu::RenderPipeline,
    geom: wgpu::RenderPipeline,
    ink: wgpu::RenderPipeline,
    fx: wgpu::RenderPipeline,
    gradient: wgpu::RenderPipeline,
    blobs: wgpu::RenderPipeline,
    panels: wgpu::RenderPipeline,
    antialias: wgpu::RenderPipeline,
}

struct Layouts {
    globals: wgpu::BindGroupLayout,
    instances: wgpu::BindGroupLayout,
    sky: wgpu::BindGroupLayout,
    ui: wgpu::BindGroupLayout,
    ink: wgpu::BindGroupLayout,
    antialias: wgpu::BindGroupLayout,
}

/// Normal, paint and depth targets with the colour target's sample coverage.
/// Ink reads individual samples; resolving encoded geometry would invent surfaces.
struct GeomTargets {
    geom: wgpu::TextureView,
    albedo: wgpu::TextureView,
    depth: wgpu::TextureView,
}

/// The renderer. Create one per window (or one headless), upload meshes once, then call
/// [`Renderer::render`] (window) or [`Renderer::render_to_rgba`] (any) with a [`Scene`].
pub struct Renderer {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: Target,
    format: wgpu::TextureFormat,
    samples: u32,
    size: (u32, u32),
    scale: f32,
    msaa: Option<wgpu::TextureView>,
    world_color: wgpu::TextureView,
    antialias_bind: wgpu::BindGroup,
    depth: wgpu::TextureView,
    geom: wgpu::TextureView,
    albedo: wgpu::TextureView,
    geom_depth: wgpu::TextureView,
    ink_uniform: wgpu::Buffer,
    ink_bind: wgpu::BindGroup,
    offscreen: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
    layouts: Layouts,
    pipelines: Pipelines,
    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    instances: Growable,
    instances_bind: wgpu::BindGroup,
    fx_vertices: Growable,
    fx_indices: Growable,
    screen: wgpu::Buffer,
    panels: Vec<Growable>,
    panels_bind: Vec<wgpu::BindGroup>,
    fallback_sky: GpuBackdrop,
    meshes: Vec<MeshSlot>,
    backdrops: Vec<Option<GpuBackdrop>>,
    text: TextSystem,
}

fn uniform_entry(binding: u32, vis: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_entry(binding: u32, samples: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: samples > 1,
        },
        count: None,
    }
}

fn storage_entry(binding: u32, vis: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffers: &[&wgpu::Buffer],
) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry> = buffers
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &entries,
    })
}

/// The discrete GPU unless `WGPU_POWER_PREF` (`low`, `high`, `none`) says otherwise. The
/// instances read `WGPU_BACKEND` (for example `gl` where Vulkan is missing) the same way.
fn power_preference() -> wgpu::PowerPreference {
    wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::HighPerformance)
}

impl Renderer {
    /// A renderer drawing into a winit window. Call [`Renderer::resize`] on resize events.
    pub fn new_windowed(window: Arc<Window>) -> Result<Self, RenderError> {
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())),
        );
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| RenderError::Surface(e.to_string()))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: power_preference(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| RenderError::Surface("no surface formats".into()))?;
        // Colours are linear and the GPU encodes them. A surface without an sRGB format
        // is drawn through an sRGB view of it, so it isn't too dark.
        let format = surface_format.add_srgb_suffix();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: if format == surface_format {
                vec![]
            } else {
                vec![format]
            },
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        let target = Target::Window {
            surface,
            config,
            window,
        };
        Self::build(
            instance,
            adapter,
            target,
            format,
            (size.width, size.height),
            scale,
        )
    }

    /// An offscreen renderer of `width` x `height` pixels, for screenshots and tests.
    pub fn new_headless(width: u32, height: u32) -> Result<Self, RenderError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: power_preference(),
            compatible_surface: None,
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        Self::build(
            instance,
            adapter,
            Target::Headless,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            (width, height),
            1.0,
        )
    }

    fn build(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        target: Target,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        scale: f32,
    ) -> Result<Self, RenderError> {
        let info = adapter.get_info();
        log::info!(
            "adapter {} ({:?}, {:?}), driver {} {}",
            info.name,
            info.backend,
            info.device_type,
            info.driver,
            info.driver_info
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("render"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| RenderError::Device(e.to_string()))?;
        let size = (size.0.max(1), size.1.max(1));
        let samples = {
            let ok = |f: wgpu::TextureFormat| {
                adapter
                    .get_texture_format_features(f)
                    .flags
                    .sample_count_supported(MSAA)
            };
            if ok(format)
                && ok(DEPTH_FORMAT)
                && ok(GEOM_FORMAT)
                && ok(wgpu::TextureFormat::Rgba8Unorm)
            {
                MSAA
            } else {
                1
            }
        };
        if let Target::Window {
            surface, config, ..
        } = &target
        {
            surface.configure(&device, config);
        }

        let vis = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let layout = |entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries,
            })
        };
        let layouts = Layouts {
            globals: layout(&[uniform_entry(0, vis)]),
            instances: layout(&[storage_entry(0, vis)]),
            sky: layout(&[uniform_entry(0, vis), storage_entry(1, vis)]),
            ui: layout(&[uniform_entry(0, vis), storage_entry(1, vis)]),
            ink: layout(&[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1, samples),
                texture_entry(2, samples),
            ]),
            antialias: layout(&[
                texture_entry(0, 1),
                uniform_entry(1, wgpu::ShaderStages::FRAGMENT),
            ]),
        };
        let pipelines = create_pipelines(&device, &layouts, format, samples);

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<GlobalsGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind = bind(&device, &layouts.globals, &[&globals]);
        let storage = wgpu::BufferUsages::STORAGE;
        let instances = Growable::new(&device, "instances", storage, 64 * 1024);
        let instances_bind = bind(&device, &layouts.instances, &[&instances.buf]);
        let screen = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let panels = Vec::new();
        let panels_bind = Vec::new();
        let fallback_sky = make_backdrop(
            &device,
            &layouts.sky,
            Backdrop {
                tile: 1024.0,
                top: defaults::SKY_TOP,
                bottom: defaults::SKY_BOTTOM,
                clouds: vec![],
                stars: vec![],
            },
        );
        let text = TextSystem::new(&device, &queue, format, 1);
        let raster_size = world_size(size, device.limits().max_texture_dimension_2d);
        let (msaa, depth) = create_targets(&device, format, samples, raster_size);
        let (world_color, antialias_bind) =
            create_world_color(&device, &layouts.antialias, &screen, format, raster_size);
        let geom_targets = create_geom_targets(&device, raster_size, samples);
        let ink_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ink"),
            contents: bytemuck::bytes_of(&InkGpu {
                params: Ink::default().uniforms(),
                color: Ink::default().color.to_array(),
                raster: [raster_size.0 as f32 / size.0 as f32, 0.0, 0.0, 0.0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let ink_bind = ink_bind_group(
            &device,
            &layouts.ink,
            &ink_uniform,
            &geom_targets.geom,
            &geom_targets.albedo,
        );
        Ok(Self {
            instance,
            adapter,
            fx_vertices: Growable::new(
                &device,
                "fx vertices",
                wgpu::BufferUsages::VERTEX,
                64 * 1024,
            ),
            fx_indices: Growable::new(&device, "fx indices", wgpu::BufferUsages::INDEX, 16 * 1024),
            device,
            queue,
            target,
            format,
            samples,
            size,
            scale,
            msaa,
            world_color,
            antialias_bind,
            depth,
            geom: geom_targets.geom,
            albedo: geom_targets.albedo,
            geom_depth: geom_targets.depth,
            ink_uniform,
            ink_bind,
            offscreen: None,
            layouts,
            pipelines,
            globals,
            globals_bind,
            instances,
            instances_bind,
            screen,
            panels,
            panels_bind,
            fallback_sky,
            meshes: Vec::new(),
            backdrops: Vec::new(),
            text,
        })
    }

    /// Size of the render target in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// UI scale (the window's DPI factor; 1 for headless unless set).
    pub fn scale_factor(&self) -> f32 {
        self.scale
    }

    /// Size in logical pixels: the coordinate space of the overlay and of
    /// [`crate::Camera::world_to_screen`] when you pass this as the viewport.
    pub fn logical_size(&self) -> Vec2 {
        Vec2::new(self.size.0 as f32, self.size.1 as f32) / self.scale
    }

    /// Human-readable adapter name and backend, for logs.
    pub fn adapter_info(&self) -> String {
        let i = self.adapter.get_info();
        format!("{} ({:?})", i.name, i.backend)
    }

    /// Changes the UI scale (DPI factor). The window renderer should forward
    /// `ScaleFactorChanged` here.
    pub fn set_scale_factor(&mut self, scale: f32) {
        if scale.is_finite() && scale > 0.0 {
            self.scale = scale;
        }
    }

    /// Resizes the target. Zero sizes (a minimised window) are ignored.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width, height) == self.size {
            return;
        }
        self.size = (width, height);
        if let Target::Window {
            surface, config, ..
        } = &mut self.target
        {
            config.width = width;
            config.height = height;
            surface.configure(&self.device, config);
        }
        let raster_size = world_size(self.size, self.device.limits().max_texture_dimension_2d);
        let (msaa, depth) = create_targets(&self.device, self.format, self.samples, raster_size);
        self.msaa = msaa;
        self.depth = depth;
        (self.world_color, self.antialias_bind) = create_world_color(
            &self.device,
            &self.layouts.antialias,
            &self.screen,
            self.format,
            raster_size,
        );
        let geom = create_geom_targets(&self.device, raster_size, self.samples);
        self.ink_bind = ink_bind_group(
            &self.device,
            &self.layouts.ink,
            &self.ink_uniform,
            &geom.geom,
            &geom.albedo,
        );
        self.geom = geom.geom;
        self.albedo = geom.albedo;
        self.geom_depth = geom.depth;
    }

    /// Uploads a mesh. Meshes stay on the GPU until [`Renderer::remove_mesh`].
    pub fn upload_mesh(&mut self, mesh: &MeshData) -> MeshHandle {
        let gpu = self.gpu_mesh(mesh);
        if let Some(i) = self.meshes.iter().position(|s| s.mesh.is_none()) {
            let slot = &mut self.meshes[i];
            slot.mesh = Some(gpu);
            MeshHandle {
                index: i as u32,
                generation: slot.generation,
            }
        } else {
            self.meshes.push(MeshSlot {
                generation: 0,
                mesh: Some(gpu),
            });
            MeshHandle {
                index: self.meshes.len() as u32 - 1,
                generation: 0,
            }
        }
    }

    /// The live mesh behind a handle, if the handle is still current.
    fn mesh(&self, handle: MeshHandle) -> Option<&GpuMesh> {
        self.meshes
            .get(handle.index as usize)
            .filter(|s| s.generation == handle.generation)
            .and_then(|s| s.mesh.as_ref())
    }

    /// Replaces a mesh in place (a rock being mined, a station growing).
    pub fn update_mesh(&mut self, handle: MeshHandle, mesh: &MeshData) {
        let gpu = self.gpu_mesh(mesh);
        if let Some(slot) = self.meshes.get_mut(handle.index as usize)
            && slot.generation == handle.generation
            && slot.mesh.is_some()
        {
            slot.mesh = Some(gpu);
        }
    }

    /// Frees a mesh. Instances that still use the handle are skipped, and a stale
    /// handle never frees a mesh uploaded later into the same slot.
    pub fn remove_mesh(&mut self, handle: MeshHandle) {
        if let Some(slot) = self.meshes.get_mut(handle.index as usize)
            && slot.generation == handle.generation
            && slot.mesh.take().is_some()
        {
            slot.generation = slot.generation.wrapping_add(1);
        }
    }

    fn gpu_mesh(&self, mesh: &MeshData) -> GpuMesh {
        let zero = [Vertex {
            position: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            outline_normal: [0.0, 0.0, 1.0],
            color: [0.0; 4],
            span: [0.0; 2],
            facet: 0.0,
            morph_position: [0.0; 3],
            morph_normal: [0.0, 0.0, 1.0],
            morph_color: [0.0; 4],
            bite: [0.0; 2],
        }];
        let (verts, idx): (&[Vertex], &[u32]) = if mesh.is_empty() {
            (&zero, &[0, 0, 0])
        } else {
            (&mesh.vertices, &mesh.indices)
        };
        GpuMesh {
            vertices: self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh vertices"),
                    contents: bytemuck::cast_slice(verts),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            indices: self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh indices"),
                    contents: bytemuck::cast_slice(idx),
                    usage: wgpu::BufferUsages::INDEX,
                }),
            index_count: if mesh.is_empty() {
                0
            } else {
                mesh.indices.len() as u32
            },
        }
    }

    /// Uploads a backdrop for [`Scene::backdrop`].
    pub fn upload_backdrop(&mut self, backdrop: &Backdrop) -> BackdropHandle {
        let gpu = make_backdrop(&self.device, &self.layouts.sky, backdrop.clone());
        self.backdrops.push(Some(gpu));
        BackdropHandle(self.backdrops.len() as u32 - 1)
    }

    pub fn remove_backdrop(&mut self, handle: BackdropHandle) {
        if let Some(slot) = self.backdrops.get_mut(handle.0 as usize) {
            *slot = None;
        }
    }

    /// Laid-out size of a text run in logical pixels (for sizing panels around it).
    pub fn measure_text(&mut self, text: &Text) -> Vec2 {
        self.text.measure(text, self.scale)
    }

    /// Characters in `text` the bundled font can't draw.
    pub fn missing_glyphs(&mut self, text: &Text) -> Vec<char> {
        self.text.missing(text, self.scale)
    }

    /// Draws a frame. For a window this presents to the screen; headless it renders into
    /// the offscreen target (use [`Renderer::render_to_rgba`] to read it back).
    pub fn render(&mut self, scene: &Scene) -> Result<(), RenderError> {
        let frame = match &mut self.target {
            Target::Headless => {
                let view = self.offscreen_view();
                let encoder = self.encode(scene, &view)?;
                self.queue.submit([encoder.finish()]);
                return Ok(());
            }
            Target::Window {
                surface,
                config,
                window,
            } => match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame) => frame,
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                    return Ok(());
                }
                wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    // The acquired texture must be released before reconfiguring.
                    drop(frame);
                    surface.configure(&self.device, config);
                    return Ok(());
                }
                wgpu::CurrentSurfaceTexture::Outdated => {
                    surface.configure(&self.device, config);
                    return Ok(());
                }
                wgpu::CurrentSurfaceTexture::Lost => {
                    *surface = self
                        .instance
                        .create_surface(window.clone())
                        .map_err(|e| RenderError::Surface(e.to_string()))?;
                    surface.configure(&self.device, config);
                    return Ok(());
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    return Err(RenderError::Surface("validation error".into()));
                }
            },
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.format),
            ..Default::default()
        });
        let encoder = self.encode(scene, &view)?;
        self.queue.submit([encoder.finish()]);
        if let Target::Window { window, .. } = &self.target {
            window.pre_present_notify();
        }
        self.queue.present(frame);
        self.text.trim();
        Ok(())
    }

    /// Renders a frame offscreen at the current size and reads it back as sRGB RGBA.
    /// Works for both windowed and headless renderers.
    pub fn render_to_rgba(&mut self, scene: &Scene) -> Result<RgbaImage, RenderError> {
        let view = self.offscreen_view();
        let mut encoder = self.encode(scene, &view)?;
        let (w, h) = self.size;
        let row = w * 4;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let texture = &self.offscreen.as_ref().expect("offscreen target").0;
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        rx.recv()
            .map_err(|e| RenderError::Readback(e.to_string()))?
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        let data = slice
            .get_mapped_range()
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        let mut pixels = Vec::with_capacity((row * h) as usize);
        for y in 0..h {
            let start = (y * padded) as usize;
            pixels.extend_from_slice(&data[start..start + row as usize]);
        }
        drop(data);
        readback.unmap();
        if matches!(
            self.format,
            wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
        ) {
            for px in pixels.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
        }
        self.text.trim();
        Ok(RgbaImage {
            width: w,
            height: h,
            pixels,
        })
    }

    fn offscreen_view(&mut self) -> wgpu::TextureView {
        let size = self.size;
        if self.offscreen.as_ref().is_none_or(|o| o.2 != size) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("offscreen"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.offscreen = Some((texture, view, size));
        }
        self.offscreen
            .as_ref()
            .map(|o| o.1.clone())
            .expect("just created")
    }

    /// Uploads per-frame data and records the world and overlay passes.
    fn encode(
        &mut self,
        scene: &Scene,
        target: &wgpu::TextureView,
    ) -> Result<wgpu::CommandEncoder, RenderError> {
        let (w, h) = self.size;
        let viewport = Vec2::new(w as f32, h as f32);
        let logical = viewport / self.scale;
        let cam = &scene.camera;

        // Globals.
        let lights = &scene.lights;
        let mut points = [[0.0; 4]; MAX_POINT_LIGHTS * 2];
        let n_points = lights.points.len().min(MAX_POINT_LIGHTS);
        for (i, p) in lights.points.iter().take(MAX_POINT_LIGHTS).enumerate() {
            points[i * 2] = p.position.extend(p.radius.max(1e-3)).into();
            points[i * 2 + 1] = p.color.to_array();
        }
        let fwd = (cam.target - cam.eye).normalize_or(glam::Vec3::NEG_Z);
        let ortho = matches!(cam.projection, Projection::Orthographic { .. });
        let globals = GlobalsGpu {
            view_proj: cam.view_proj(viewport.x / viewport.y).to_cols_array_2d(),
            camera_pos: cam.eye.extend(scene.time).into(),
            camera_fwd: fwd.extend(if ortho { 1.0 } else { 0.0 }).into(),
            viewport: [viewport.x, viewport.y, self.scale, 0.0],
            sun_dir: lights
                .sun_direction
                .normalize_or(glam::Vec3::Z)
                .extend(0.0)
                .into(),
            sun_color: lights.sun_color.to_array(),
            ambient: lights.ambient.to_array(),
            rim: lights.rim.to_array(),
            // y: the tone bands' fade width in half-Lambert units. Near the terminator
            // half-Lambert changes by about half a radian's worth per radian of turn.
            counts: [
                n_points as f32,
                0.5 * lights.band_fade_degrees.clamp(0.0, 90.0).to_radians(),
                0.0,
                0.0,
            ],
            points,
        };
        self.queue
            .write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        let mut ink = scene.ink.uniforms();
        ink[3] = (ink[3] * self.scale).round().clamp(1.0, 3.0);
        self.queue.write_buffer(
            &self.ink_uniform,
            0,
            bytemuck::bytes_of(&InkGpu {
                params: ink,
                color: scene.ink.color.to_array(),
                raster: [
                    world_size(self.size, self.device.limits().max_texture_dimension_2d).0 as f32
                        / w as f32,
                    0.0,
                    0.0,
                    0.0,
                ],
            }),
        );

        // Mesh instances, grouped by mesh.
        let mut order: Vec<usize> = (0..scene.meshes.len())
            .filter(|&i| {
                let m = &scene.meshes[i];
                m.transform.is_finite() && self.mesh(m.mesh).is_some_and(|g| g.index_count > 0)
            })
            .collect();
        order.sort_by_key(|&i| scene.meshes[i].mesh.index);
        let mut gpu_instances = Vec::with_capacity(order.len());
        let mut runs: Vec<(u32, u32, u32)> = Vec::new();
        for &i in &order {
            let inst = &scene.meshes[i];
            let model = inst.transform;
            let m3 = Mat3::from_mat4(model);
            let normal = if m3.determinant().abs() > 1e-12 {
                Mat4::from_mat3(m3.inverse().transpose())
            } else {
                model
            };
            let mat = &inst.material;
            gpu_instances.push(InstanceGpu {
                model: model.to_cols_array_2d(),
                normal: normal.to_cols_array_2d(),
                tint: mat.tint.to_array(),
                shadow: [
                    mat.shadow.r,
                    mat.shadow.g,
                    mat.shadow.b,
                    mat.bands.clamp(2, 3) as f32,
                ],
                highlight: [
                    mat.highlight.r,
                    mat.highlight.g,
                    mat.highlight.b,
                    mat.hatch.clamp(0.0, 1.0),
                ],
                outline: [
                    mat.outline.r,
                    mat.outline.g,
                    mat.outline.b,
                    mat.outline_px.clamp(0.0, 32.0) * self.scale,
                ],
                emissive: mat.emissive.to_array(),
                morph: [inst.morph, inst.hatch_scale, 0.0, 0.0],
            });
            let k = gpu_instances.len() as u32 - 1;
            match runs.last_mut() {
                Some(r) if r.0 == inst.mesh.index => r.2 += 1,
                _ => runs.push((inst.mesh.index, k, 1)),
            }
        }
        if self.instances.write(
            &self.device,
            &self.queue,
            bytemuck::cast_slice(&gpu_instances),
        ) {
            self.instances_bind = bind(
                &self.device,
                &self.layouts.instances,
                &[&self.instances.buf],
            );
        }

        // Lines and glows.
        let (mut fxv, mut fxi): (Vec<FxVertex>, Vec<u32>) = (Vec::new(), Vec::new());
        fx::tessellate(&scene.lines, &scene.glows, cam, &mut fxv, &mut fxi);
        self.fx_vertices
            .write(&self.device, &self.queue, bytemuck::cast_slice(&fxv));
        self.fx_indices
            .write(&self.device, &self.queue, bytemuck::cast_slice(&fxi));

        // Backdrop.
        let sky = scene
            .backdrop
            .and_then(|b| self.backdrops.get(b.0 as usize))
            .and_then(Option::as_ref)
            .unwrap_or(&self.fallback_sky);
        let src = &sky.source;
        let tile = src.tile;
        let sky_gpu = SkyGpu {
            top: src.top.to_array(),
            bottom: src.bottom.to_array(),
            clouds: [
                cam.target.x * CLOUD_PARALLAX,
                -cam.target.y * CLOUD_PARALLAX,
                tile,
                self.scale,
            ],
            stars: [
                cam.target.x * STAR_PARALLAX,
                -cam.target.y * STAR_PARALLAX,
                0.0,
                0.0,
            ],
            counts: [
                (logical.x / tile).ceil() + 2.0,
                (logical.y / tile).ceil() + 2.0,
                src.clouds.len() as f32,
                src.stars.len() as f32,
            ],
            viewport: [viewport.x, viewport.y, scene.time, 0.0],
        };
        self.queue
            .write_buffer(&sky.sky, 0, bytemuck::bytes_of(&sky_gpu));
        let sky_blobs = (src.clouds.len() + src.stars.len()) as u32;
        let sky_instances = sky_blobs * sky_gpu.counts[0] as u32 * sky_gpu.counts[1] as u32;

        // Overlay: layer 0, then each upper layer above it.
        let overlay = &scene.overlay;
        let panel_layers: Vec<&[Panel]> = std::iter::once(overlay.panels.as_slice())
            .chain(
                overlay
                    .widgets
                    .iter()
                    .chain(&overlay.upper)
                    .map(|l| l.panels.as_slice()),
            )
            .collect();
        let text_layers: Vec<&[Text]> = std::iter::once(overlay.texts.as_slice())
            .chain(
                overlay
                    .widgets
                    .iter()
                    .chain(&overlay.upper)
                    .map(|l| l.texts.as_slice()),
            )
            .collect();
        while self.panels.len() < panel_layers.len() {
            let buffer = Growable::new(&self.device, "panels", wgpu::BufferUsages::STORAGE, 4096);
            self.panels_bind.push(bind(
                &self.device,
                &self.layouts.ui,
                &[&self.screen, &buffer.buf],
            ));
            self.panels.push(buffer);
        }
        let mut panel_counts = vec![0u32; panel_layers.len()];
        for (i, panels) in panel_layers.iter().enumerate() {
            let data: Vec<PanelGpu> = panels.iter().map(|p| panel_gpu(p, self.scale)).collect();
            panel_counts[i] = data.len() as u32;
            if self.panels[i].write(&self.device, &self.queue, bytemuck::cast_slice(&data)) {
                self.panels_bind[i] = bind(
                    &self.device,
                    &self.layouts.ui,
                    &[&self.screen, &self.panels[i].buf],
                );
            }
        }
        self.queue.write_buffer(
            &self.screen,
            0,
            bytemuck::cast_slice(&[viewport.x, viewport.y, self.scale, 0.0]),
        );
        self.text
            .prepare(
                &self.device,
                &self.queue,
                self.size,
                self.scale,
                &text_layers,
            )
            .map_err(|e| RenderError::Text(e.to_string()))?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        let clear = scene.clear_color;
        let (world_target, resolve) = match &self.msaa {
            Some(msaa) => (msaa, Some(&self.world_color)),
            None => (&self.world_color, None),
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: world_target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear.r as f64,
                            g: clear.g as f64,
                            b: clear.b as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if scene.backdrop.is_some() {
                pass.set_pipeline(&self.pipelines.gradient);
                pass.set_bind_group(0, &sky.bind, &[]);
                pass.draw(0..3, 0..1);
                if sky_instances > 0 {
                    pass.set_pipeline(&self.pipelines.blobs);
                    pass.draw(0..6, 0..sky_instances);
                }
            }

            pass.set_bind_group(0, &self.globals_bind, &[]);
            if !runs.is_empty() {
                pass.set_bind_group(1, &self.instances_bind, &[]);
                for pipeline in [&self.pipelines.mesh, &self.pipelines.outline] {
                    pass.set_pipeline(pipeline);
                    for &(mesh, start, count) in &runs {
                        let Some(m) = self.meshes.get(mesh as usize).and_then(|s| s.mesh.as_ref())
                        else {
                            continue;
                        };
                        pass.set_vertex_buffer(0, m.vertices.slice(..));
                        pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..m.index_count, 0, start..start + count);
                    }
                }
            }
        }
        // Keep sample coverage through geometry, ink and effects. Resolve only
        // the finished colour, never the encoded normals, depth or paint.
        if !runs.is_empty() || !fxi.is_empty() {
            let clear_geom = wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                }),
                store: wgpu::StoreOp::Store,
            };
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("geom"),
                    color_attachments: &[
                        Some(wgpu::RenderPassColorAttachment {
                            view: &self.geom,
                            depth_slice: None,
                            resolve_target: None,
                            ops: clear_geom,
                        }),
                        Some(wgpu::RenderPassColorAttachment {
                            view: &self.albedo,
                            depth_slice: None,
                            resolve_target: None,
                            ops: clear_geom,
                        }),
                    ],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.geom_depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if !runs.is_empty() {
                    pass.set_pipeline(&self.pipelines.geom);
                    pass.set_bind_group(0, &self.globals_bind, &[]);
                    pass.set_bind_group(1, &self.instances_bind, &[]);
                }
                for &(mesh, start, count) in &runs {
                    let Some(m) = self.meshes.get(mesh as usize).and_then(|s| s.mesh.as_ref())
                    else {
                        continue;
                    };
                    pass.set_vertex_buffer(0, m.vertices.slice(..));
                    pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.index_count, 0, start..start + count);
                }
            }
            if !runs.is_empty() {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ink"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: world_target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.pipelines.ink);
                pass.set_bind_group(0, &self.ink_bind, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        if !fxi.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("effects"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: world_target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.geom_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines.fx);
            pass.set_bind_group(0, &self.globals_bind, &[]);
            pass.set_vertex_buffer(0, self.fx_vertices.buf.slice(..));
            pass.set_index_buffer(self.fx_indices.buf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..fxi.len() as u32, 0, 0..1);
        }
        if let Some(resolve) = resolve {
            // Also resolve empty scenes, which still contain the sky/clear colour.
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("resolve world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: world_target,
                    depth_slice: None,
                    resolve_target: Some(resolve),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("downsample world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines.antialias);
            pass.set_bind_group(0, &self.antialias_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for (layer, &count) in panel_counts.iter().enumerate() {
                if count > 0 {
                    pass.set_pipeline(&self.pipelines.panels);
                    pass.set_bind_group(0, &self.panels_bind[layer], &[]);
                    pass.draw(0..6, 0..count);
                }
                self.text
                    .render(layer, &mut pass)
                    .map_err(|e| RenderError::Text(e.to_string()))?;
            }
        }
        Ok(encoder)
    }
}

fn panel_gpu(p: &Panel, scale: f32) -> PanelGpu {
    let r = p.rect;
    PanelGpu {
        rect: [
            r.x * scale,
            r.y * scale,
            r.w.max(0.0) * scale,
            r.h.max(0.0) * scale,
        ],
        params: [
            p.radius.max(0.0) * scale,
            p.border_px.max(0.0) * scale,
            if p.shadow { 5.0 * scale } else { 0.0 },
            p.dash.max(0.0) * scale,
        ],
        // Straight alpha; the shader premultiplies.
        fill: p.fill.to_array(),
        border: p.border.to_array(),
        shape: [
            p.rotate,
            match p.shape {
                PanelShape::Box => 0.0,
                PanelShape::Triangle => 1.0,
            },
            0.0,
            0.0,
        ],
    }
}

fn make_backdrop(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: Backdrop,
) -> GpuBackdrop {
    let mut blobs: Vec<BlobGpu> = source
        .clouds
        .iter()
        .chain(&source.stars)
        .map(|b| BlobGpu {
            pos: [b.pos.x, b.pos.y, b.radius, 0.0],
            color: b.color.to_array(),
        })
        .collect();
    if blobs.is_empty() {
        blobs.push(BlobGpu::zeroed());
    }
    let blob_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("backdrop blobs"),
        contents: bytemuck::cast_slice(&blobs),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let sky = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sky"),
        size: std::mem::size_of::<SkyGpu>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind = bind(device, layout, &[&sky, &blob_buf]);
    GpuBackdrop {
        source,
        sky,
        _blobs: blob_buf,
        bind,
    }
}

fn create_targets(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    samples: u32,
    size: (u32, u32),
) -> (Option<wgpu::TextureView>, wgpu::TextureView) {
    let extent = wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    };
    let make = |format, label| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    };
    let msaa = (samples > 1).then(|| make(format, "msaa colour"));
    (msaa, make(DEPTH_FORMAT, "depth"))
}

// Keep the public/output size native; only world attachments use this denser raster.
fn world_size(size: (u32, u32), limit: u32) -> (u32, u32) {
    let scale = if size.0 <= limit / 2 && size.1 <= limit / 2 {
        2
    } else {
        1
    };
    (size.0 * scale, size.1 * scale)
}

fn create_world_color(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    screen: &wgpu::Buffer,
    format: wgpu::TextureFormat,
    size: (u32, u32),
) -> (wgpu::TextureView, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("resolved world colour"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("downsample world"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: screen.as_entire_binding(),
            },
        ],
    });
    (view, bind)
}

fn create_geom_targets(device: &wgpu::Device, size: (u32, u32), samples: u32) -> GeomTargets {
    let extent = wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    };
    let color = |label: &str, format| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    };
    GeomTargets {
        geom: color("geom normals", GEOM_FORMAT),
        albedo: color("geom paint", wgpu::TextureFormat::Rgba8Unorm),
        depth: device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("geom depth"),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default()),
    }
}

fn ink_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    geom: &wgpu::TextureView,
    albedo: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ink"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(geom),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(albedo),
            },
        ],
    })
}

fn ink_shader_source(samples: u32) -> String {
    let source = include_str!("../shaders/ink.wgsl");
    if samples > 1 {
        source.into()
    } else {
        // The last textureLoad argument is the sample for MSAA, mip 0 otherwise.
        // Keep the edge detector shared with adapters without multisampling.
        format!(
            "const sample: u32 = 0u;\n{}",
            source
                .replace("texture_multisampled_2d", "texture_2d")
                .replace(", @builtin(sample_index) sample: u32", "")
        )
    }
}

fn create_pipelines(
    device: &wgpu::Device,
    layouts: &Layouts,
    format: wgpu::TextureFormat,
    samples: u32,
) -> Pipelines {
    let globals = include_str!("../shaders/globals.wgsl");
    let module = |label: &str, src: String| {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(src.into()),
        })
    };
    let mesh_src = module(
        "mesh.wgsl",
        format!("{globals}\n{}", include_str!("../shaders/mesh.wgsl")),
    );
    let fx_src = module(
        "fx.wgsl",
        format!("{globals}\n{}", include_str!("../shaders/fx.wgsl")),
    );
    let sky_src = module(
        "backdrop.wgsl",
        include_str!("../shaders/backdrop.wgsl").into(),
    );
    let ui_src = module("ui.wgsl", include_str!("../shaders/ui.wgsl").into());
    let ink_src = module("ink.wgsl", ink_shader_source(samples));
    let antialias_src = module(
        "antialias.wgsl",
        include_str!("../shaders/antialias.wgsl").into(),
    );

    let pl = |layouts: &[Option<&wgpu::BindGroupLayout>]| {
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: layouts,
            immediate_size: 0,
        })
    };
    let mesh_layout = pl(&[Some(&layouts.globals), Some(&layouts.instances)]);
    let fx_layout = pl(&[Some(&layouts.globals)]);
    let sky_layout = pl(&[Some(&layouts.sky)]);
    let ui_layout = pl(&[Some(&layouts.ui)]);
    let ink_layout = pl(&[Some(&layouts.ink)]);
    let antialias_layout = pl(&[Some(&layouts.antialias)]);

    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x4, 4 => Float32x2,
            5 => Float32, 6 => Float32x3, 7 => Float32x3, 8 => Float32x4, 9 => Float32x2
        ],
    };
    let fx_vertex_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<FxVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4
        ],
    };
    let depth = |write: bool, compare: wgpu::CompareFunction| {
        Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(write),
            depth_compare: Some(compare),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        })
    };
    let premul = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
    struct Spec<'a> {
        label: &'a str,
        layout: &'a wgpu::PipelineLayout,
        module: &'a wgpu::ShaderModule,
        vs: &'a str,
        fs: &'a str,
        buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
        cull: Option<wgpu::Face>,
        depth: Option<wgpu::DepthStencilState>,
        blend: Option<wgpu::BlendState>,
        samples: u32,
        /// Colour formats. One for the usual passes; two unorm targets for the geom pass.
        formats: &'a [wgpu::TextureFormat],
    }
    let make = |s: Spec| {
        let targets: Vec<_> = s
            .formats
            .iter()
            .map(|format| {
                Some(wgpu::ColorTargetState {
                    format: *format,
                    blend: s.blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })
            })
            .collect();
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(s.label),
            layout: Some(s.layout),
            vertex: wgpu::VertexState {
                module: s.module,
                entry_point: Some(s.vs),
                compilation_options: Default::default(),
                buffers: s.buffers,
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: s.cull,
                ..Default::default()
            },
            depth_stencil: s.depth,
            multisample: wgpu::MultisampleState {
                count: s.samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: s.module,
                entry_point: Some(s.fs),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let surface = [format];
    let geom_formats = [GEOM_FORMAT, wgpu::TextureFormat::Rgba8Unorm];
    let less = wgpu::CompareFunction::Less;
    Pipelines {
        antialias: make(Spec {
            label: "downsample world",
            layout: &antialias_layout,
            module: &antialias_src,
            vs: "vs_antialias",
            fs: "fs_antialias",
            buffers: &[],
            cull: None,
            depth: None,
            blend: None,
            samples: 1,
            formats: &surface,
        }),
        mesh: make(Spec {
            label: "mesh",
            layout: &mesh_layout,
            module: &mesh_src,
            vs: "vs_main",
            fs: "fs_main",
            buffers: &[Some(vertex_layout.clone())],
            cull: Some(wgpu::Face::Back),
            depth: depth(true, less),
            blend: None,
            samples,
            formats: &surface,
        }),
        outline: make(Spec {
            label: "outline",
            layout: &mesh_layout,
            module: &mesh_src,
            vs: "vs_outline",
            fs: "fs_outline",
            buffers: &[Some(vertex_layout.clone())],
            cull: Some(wgpu::Face::Front),
            depth: depth(true, less),
            blend: None,
            samples,
            formats: &surface,
        }),
        geom: make(Spec {
            label: "geom",
            layout: &mesh_layout,
            module: &mesh_src,
            vs: "vs_geom",
            fs: "fs_geom",
            buffers: &[Some(vertex_layout)],
            cull: Some(wgpu::Face::Back),
            depth: depth(true, less),
            blend: None,
            samples,
            formats: &geom_formats,
        }),
        ink: make(Spec {
            label: "ink",
            layout: &ink_layout,
            module: &ink_src,
            vs: "vs_ink",
            fs: "fs_ink",
            buffers: &[],
            cull: None,
            depth: None,
            blend: premul,
            samples,
            formats: &surface,
        }),
        fx: make(Spec {
            label: "fx",
            layout: &fx_layout,
            module: &fx_src,
            vs: "vs_fx",
            fs: "fs_fx",
            buffers: &[Some(fx_vertex_layout)],
            cull: None,
            // Effects cover inner ink and share the geometry's sample depth
            // (beams aren't depth-tested: see `vs_fx`). Resolve afterwards.
            depth: depth(false, wgpu::CompareFunction::LessEqual),
            blend: premul,
            samples,
            formats: &surface,
        }),
        gradient: make(Spec {
            label: "backdrop gradient",
            layout: &sky_layout,
            module: &sky_src,
            vs: "vs_gradient",
            fs: "fs_gradient",
            buffers: &[],
            cull: None,
            depth: depth(false, wgpu::CompareFunction::Always),
            blend: None,
            samples,
            formats: &surface,
        }),
        blobs: make(Spec {
            label: "backdrop blobs",
            layout: &sky_layout,
            module: &sky_src,
            vs: "vs_blob",
            fs: "fs_blob",
            buffers: &[],
            cull: None,
            depth: depth(false, wgpu::CompareFunction::Always),
            blend: premul,
            samples,
            formats: &surface,
        }),
        panels: make(Spec {
            label: "panels",
            layout: &ui_layout,
            module: &ui_src,
            vs: "vs_panel",
            fs: "fs_panel",
            buffers: &[],
            cull: None,
            depth: None,
            blend: premul,
            samples: 1,
            formats: &surface,
        }),
    }
}
