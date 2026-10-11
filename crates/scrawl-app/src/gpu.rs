//! Renders a Vello scene into a wgpu texture that egui can display.

use egui_wgpu::RenderState;
use vello::wgpu;
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene, peniko};

struct Target {
    view: wgpu::TextureView,
    id: egui::TextureId,
    width: u32,
    height: u32,
}

/// Which texture to render into: each surface showing a scene has its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Canvas = 0,
    Preview = 1,
}

pub struct VelloCanvas {
    renderer: Renderer,
    targets: [Option<Target>; 2],
    blurrer: scrawl_render::effects::Blurrer,
}

impl VelloCanvas {
    pub fn new(rs: &RenderState) -> Result<Self, String> {
        let renderer = Renderer::new(
            &rs.device,
            RendererOptions {
                antialiasing_support: AaSupport::area_only(),
                ..Default::default()
            },
        )
        .map_err(|e| format!("failed to create Vello renderer: {e}"))?;
        let blurrer = scrawl_render::effects::Blurrer::new(&rs.device);
        Ok(Self { renderer, targets: [None, None], blurrer })
    }

    /// Renders `scene` at `width`×`height` physical pixels into `slot`'s texture and returns the
    /// egui texture showing it.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        rs: &RenderState,
        slot: Slot,
        scene: &Scene,
        effects: &[scrawl_render::effects::Effect],
        width: u32,
        height: u32,
        background: peniko::Color,
    ) -> Option<egui::TextureId> {
        if width == 0 || height == 0 {
            return None;
        }
        self.ensure_target(rs, slot, width, height);
        let target = self.targets[slot as usize].as_ref()?;
        let (view, id) = (target.view.clone(), target.id);
        // Blurs render offscreen first; their results stand in for placeholder images.
        if let Err(e) = self.blurrer.prepare(&rs.device, &rs.queue, &mut self.renderer, effects) {
            log::error!("{e}");
        }
        let params = RenderParams { base_color: background, width, height, antialiasing_method: AaConfig::Area };
        let result = self.renderer.render_to_texture(&rs.device, &rs.queue, scene, &view, &params);
        self.blurrer.finish(&mut self.renderer);
        if let Err(e) = result {
            log::error!("canvas render failed: {e}");
            return None;
        }
        Some(id)
    }

    fn ensure_target(&mut self, rs: &RenderState, slot: Slot, width: u32, height: u32) {
        let target = &mut self.targets[slot as usize];
        if target.as_ref().is_some_and(|t| t.width == width && t.height == height) {
            return;
        }
        let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scrawl canvas"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut egui_renderer = rs.renderer.write();
        let id = match target.take() {
            Some(old) => {
                egui_renderer.update_egui_texture_from_wgpu_texture(&rs.device, &view, wgpu::FilterMode::Nearest, old.id);
                old.id
            }
            None => egui_renderer.register_native_texture(&rs.device, &view, wgpu::FilterMode::Nearest),
        };
        *target = Some(Target { view, id, width, height });
    }
}
