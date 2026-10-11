//! Blurs, which a single Vello pass can't do. While a page is turned into a scene, each blurred
//! layer records an [`Effect`]: a scene of what to blur (in target pixels, from the effect's
//! corner) and a blank placeholder image drawn where the result goes. Before the main scene is
//! rendered, [`Blurrer::prepare`] renders each effect's scene offscreen, blurs it with a
//! separable Gaussian compute shader, and has Vello use that texture for the placeholder.

use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};
use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, Scene};

/// Something to render, blur and composite.
pub struct Effect {
    /// Drawn in the main scene where the blurred result goes.
    pub image: ImageData,
    /// What to blur, in pixels from the effect's top-left.
    pub scene: Scene,
    /// The Gaussian's standard deviation, in pixels.
    pub sigma: f32,
    /// What the scene is drawn over before blurring: transparent for layer blurs, opaque for
    /// background blurs (whose result must hide what's beneath).
    pub backdrop: vello::peniko::Color,
}

impl Effect {
    /// An effect `width`×`height` pixels with a fresh placeholder.
    pub fn new(scene: Scene, width: u32, height: u32, sigma: f32) -> Self {
        // The placeholder's pixels are never read (its texture is overridden), but must match
        // its size; zeroed memory is cheap.
        let data = Blob::from(vec![0u8; (width * height * 4) as usize]);
        let image = ImageData { data, format: ImageFormat::Rgba8, alpha_type: ImageAlphaType::AlphaPremultiplied, width, height };
        Self { image, scene, sigma, backdrop: vello::peniko::Color::TRANSPARENT }
    }
}

/// The largest effect side, in pixels; bigger ones are skipped (drawn unblurred).
pub const MAX_SIDE: u32 = 4096;

const SHADER: &str = r#"
struct Params { direction: vec2<i32>, radius: i32, sigma: f32 };
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(textureDimensions(src));
    let at = vec2<i32>(id.xy);
    if (at.x >= size.x || at.y >= size.y) { return; }
    var acc = vec4<f32>(0.0);
    var total = 0.0;
    for (var i = -params.radius; i <= params.radius; i = i + 1) {
        let w = exp(-f32(i * i) / (2.0 * params.sigma * params.sigma));
        let p = at + params.direction * i;
        // Outside the texture counts as transparent.
        if (p.x >= 0 && p.y >= 0 && p.x < size.x && p.y < size.y) {
            acc = acc + textureLoad(src, p, 0) * w;
        }
        total = total + w;
    }
    textureStore(dst, at, acc / total);
}
"#;

/// The blur compute pipeline, and the textures of the effects of the frame being rendered.
pub struct Blurrer {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// Kept until [`Blurrer::finish`], so overrides stay valid through the main render.
    live: Vec<(ImageData, wgpu::Texture)>,
}

impl Blurrer {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("blur"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("blur"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("blur"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { pipeline, layout, live: vec![] }
    }

    /// Renders and blurs `effects` in order (later ones may show earlier ones' results), and has
    /// `renderer` draw each result in place of its placeholder.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, renderer: &mut Renderer, effects: &[Effect]) -> Result<(), String> {
        for e in effects {
            let (w, h) = (e.image.width, e.image.height);
            if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
                continue;
            }
            let texture = |label, copy: bool| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::STORAGE_BINDING
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | if copy { wgpu::TextureUsages::COPY_SRC } else { wgpu::TextureUsages::empty() },
                    view_formats: &[],
                })
            };
            let (a, b) = (texture("effect", true), texture("effect blur", false));
            let (va, vb) = (a.create_view(&Default::default()), b.create_view(&Default::default()));
            let params = RenderParams { base_color: e.backdrop, width: w, height: h, antialiasing_method: AaConfig::Area };
            renderer.render_to_texture(device, queue, &e.scene, &va, &params).map_err(|err| format!("effect render failed: {err}"))?;
            // Horizontal into `b`, then vertical back into `a`.
            let radius = (e.sigma * 3.0).ceil().clamp(1.0, 512.0) as i32;
            let mut encoder = device.create_command_encoder(&Default::default());
            for (src, dst, dir) in [(&va, &vb, [1i32, 0]), (&vb, &va, [0, 1])] {
                let mut bytes = Vec::with_capacity(16);
                bytes.extend_from_slice(&dir[0].to_le_bytes());
                bytes.extend_from_slice(&dir[1].to_le_bytes());
                bytes.extend_from_slice(&radius.to_le_bytes());
                bytes.extend_from_slice(&e.sigma.max(0.5).to_le_bytes());
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("blur params"),
                    size: 16,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&buffer, 0, &bytes);
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("blur"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(dst) },
                        wgpu::BindGroupEntry { binding: 2, resource: buffer.as_entire_binding() },
                    ],
                });
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
            }
            queue.submit([encoder.finish()]);
            renderer.override_image(
                &e.image,
                Some(wgpu::TexelCopyTextureInfoBase { texture: a.clone(), mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }),
            );
            self.live.push((e.image.clone(), a));
        }
        Ok(())
    }

    /// Drops the frame's overrides and textures, after the main scene is rendered.
    pub fn finish(&mut self, renderer: &mut Renderer) {
        for (image, _) in self.live.drain(..) {
            renderer.override_image(&image, None);
        }
    }
}
