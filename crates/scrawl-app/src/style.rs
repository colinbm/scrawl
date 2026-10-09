use egui::Color32;

/// Selection and highlight color.
pub const ACCENT: Color32 = Color32::from_rgb(0x1F, 0x8F, 0xFF);

pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(6.0, 3.0);
        style.spacing.interact_size.y = 20.0;
        style.visuals.selection.bg_fill = ACCENT.gamma_multiply(if style.visuals.dark_mode { 0.45 } else { 0.25 });
        style.visuals.selection.stroke.color = if style.visuals.dark_mode { Color32::WHITE } else { Color32::from_rgb(0x0A, 0x3D, 0x7A) };
    });
}
