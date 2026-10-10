//! Converts between Scrawl and Sketch files, by extension:
//! `cargo run -p scrawl-core --example sketch -- in.scrawl out.sketch` (or the other way round).

use std::path::Path;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let [_, input, output] = &args[..] else {
        return Err("usage: sketch IN.(scrawl|sketch) OUT.(sketch|scrawl)".into());
    };
    let is_sketch = |p: &str| p.ends_with(".sketch");
    let doc = if is_sketch(input) {
        scrawl_core::sketch::import(&std::fs::read(input).map_err(|e| e.to_string())?)?
    } else {
        scrawl_core::io::load(Path::new(input)).map_err(|e| e.to_string())?
    };
    if is_sketch(output) {
        std::fs::write(output, scrawl_core::sketch::export(&doc)?).map_err(|e| e.to_string())?;
    } else {
        scrawl_core::io::save(&doc, Path::new(output)).map_err(|e| e.to_string())?;
    }
    println!("wrote {output}");
    Ok(())
}
