//! Renders the first page of a document to PNG without a window:
//! `cargo run -p scrawl-render --example render -- in.scrawl out.png [scale]`.

use scrawl_render::export::Exporter;
use vello::peniko::Color;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let [_, input, output, rest @ ..] = &args[..] else {
        return Err("usage: render IN.scrawl OUT.png [scale]".into());
    };
    let scale: f64 = rest.first().map_or(Ok(2.0), |s| s.parse()).map_err(|_| "bad scale")?;
    let doc = scrawl_core::io::load(std::path::Path::new(input)).map_err(|e| e.to_string())?;
    let page = &doc.pages[0];
    let area = page.content_bounds().ok_or("page is empty")?.inflate(40.0, 40.0);
    let mut exporter = Exporter::new()?;
    let image = exporter.render_page(page, area, scale, Color::new([0.93, 0.93, 0.94, 1.0]))?;
    image.write_png(std::path::Path::new(output))?;
    println!("wrote {output} ({}×{})", image.width, image.height);
    Ok(())
}
