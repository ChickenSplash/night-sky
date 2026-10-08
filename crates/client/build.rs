use std::{env, fs, path::PathBuf};

fn main() {
    // Bundle the same frontend sources, with no filesystem/network access at runtime.
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../web");
    let read = |name: &str| {
        let path = root.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        fs::read_to_string(path).unwrap()
    };
    let css = read("ui/panels.css");
    let js = [
        "ui/bridge.js",
        "ui/panels.js",
        "ui/annotations.js",
        "ui/native.js",
    ]
    .map(|name| read(name).replace("export function ", "function "))
    .join("\n");
    let html = read("native.html")
        .replace("/* SHARED_CSS */", &css)
        .replace("/* SHARED_JS */", &js);
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("native-ui.html"),
        html,
    )
    .unwrap();
}
