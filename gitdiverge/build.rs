use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let webclient = Path::new(&manifest)
        .join("..")
        .join("webclientsrc")
        .join("dist");

    // Silence "unexpected_cfgs" warnings on newer compilers while remaining
    // compatible with our MSRV (unknown directives are ignored by older Cargo).
    println!("cargo:rustc-check-cfg=cfg(webclient_present)");

    println!("cargo:rerun-if-changed={}", webclient.display());

    if webclient.exists() {
        if let Ok(mut entries) = std::fs::read_dir(&webclient) {
            if entries.next().is_some() {
                println!("cargo:rustc-cfg=webclient_present");
            }
        }
    }
}
