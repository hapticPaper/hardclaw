use std::env;
use std::path::Path;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let icon_path = Path::new("docs/assets/favicon.ico");
    if !icon_path.exists() {
        println!(
            "cargo:warning=Windows icon not found at {}",
            icon_path.display()
        );
        return;
    }

    let mut res = winres::WindowsResource::new();
    res.set_icon(icon_path.to_str().unwrap());
    res.compile().expect("failed to compile Windows resources");
}
