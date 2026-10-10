//! Headless rendering to RGBA pixels and PNG, without a window: used for export and for checking
//! rendering in tests and scripts.

use scrawl_core::Page;
use scrawl_core::kurbo::{Affine, Rect};
use vello::peniko;
use vello::wgpu;
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

/// A GPU device and Vello renderer for offscreen rendering.
pub struct Exporter {
    pub text: crate::TextEngine,
    /// Decoded images; load a document's assets into it before rendering its pages.
    pub images: crate::ImageCache,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
}

/// Straight (non-premultiplied) RGBA8 pixels, row-major.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Exporter {
    pub fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU adapter: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("scrawl export"),
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU device: {e}"))?;
        let renderer = Renderer::new(
            &device,
            RendererOptions { antialiasing_support: AaSupport::area_only(), num_init_threads: None, ..Default::default() },
        )
        .map_err(|e| format!("failed to create renderer: {e}"))?;
        Ok(Self { text: crate::TextEngine::new(), images: Default::default(), device, queue, renderer })
    }

    /// Renders `area` (page space) of `page` at `scale` pixels per unit over `background`.
    pub fn render_page(&mut self, page: &Page, area: Rect, scale: f64, background: peniko::Color) -> Result<Image, String> {
        let width = (area.width() * scale).ceil().max(1.0) as u32;
        let height = (area.height() * scale).ceil().max(1.0) as u32;
        let mut scene = Scene::new();
        let view = Affine::scale(scale) * Affine::translate(-area.origin().to_vec2());
        crate::render_page(&mut scene, page, view, &mut self.text, &self.images);
        self.render_scene(&scene, width, height, background)
    }

    pub fn render_scene(&mut self, scene: &Scene, width: u32, height: u32, background: peniko::Color) -> Result<Image, String> {
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("export target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let params = RenderParams { base_color: background, width, height, antialiasing_method: AaConfig::Area };
        self.renderer
            .render_to_texture(&self.device, &self.queue, scene, &view, &params)
            .map_err(|e| format!("render failed: {e}"))?;

        // Rows in the readback buffer must be 256-byte aligned.
        let row = width * 4;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("export readback"),
            size: padded as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: None },
            },
            size,
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("readback failed: {e}"))?;

        let data = slice.get_mapped_range().map_err(|e| format!("readback failed: {e:?}"))?;
        let mut pixels = Vec::with_capacity((row * height) as usize);
        for y in 0..height as usize {
            let start = y * padded as usize;
            pixels.extend_from_slice(&data[start..start + row as usize]);
        }
        drop(data);
        buffer.unmap();
        // Vello writes premultiplied alpha; PNG wants straight alpha.
        for px in pixels.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            if a > 0 && a < 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        Ok(Image { width, height, pixels })
    }
}

impl Image {
    pub fn write_png(&self, path: &std::path::Path) -> Result<(), String> {
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&self.pixels).map_err(|e| e.to_string())
    }
}
