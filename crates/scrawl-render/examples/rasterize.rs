//! Renders an SVG file to a square PNG: `rasterize in.svg out.png 512`.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, size] = args.as_slice() else {
        return Err("usage: rasterize in.svg out.png SIZE".into());
    };
    let size: u32 = size.parse()?;
    let rgba = scrawl_render::svg::rasterize(&std::fs::read(input)?, size)?;
    let image = image::RgbaImage::from_raw(size, size, rgba).ok_or("bad size")?;
    image.save(output)?;
    Ok(())
}
