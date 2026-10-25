//! cargo run -p alloy-core --example inspect_update -- <file.alloyupdate> [current-version]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let current = args.get(2).map(String::as_str).unwrap_or("0.1.0");
    match alloy_core::updates::inspect(std::path::Path::new(&args[1]), current) {
        Ok(u) => println!("VALID  {} -> {} (newer: {})", u.current, u.manifest.version, u.newer),
        Err(e) => println!("REJECTED  {e}"),
    }
}
