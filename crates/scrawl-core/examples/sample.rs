//! Writes the sample document: `cargo run -p scrawl-core --example sample [out.scrawl]`.

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "sample.scrawl".into());
    scrawl_core::io::save(&scrawl_core::sample::sample_document(), std::path::Path::new(&out)).expect("save sample");
    println!("wrote {out}");
}
