use std::{env, fs, path::PathBuf};

fn main() {
    let source = PathBuf::from("kernel/src/index.ts");
    println!("cargo::rerun-if-changed={}", source.display());
    let code = fs::read_to_string(&source).expect("read kernel TypeScript");
    // The M2 kernel intentionally uses the JavaScript subset of TypeScript.
    // Keeping this boundary in one build script lets the rolldown integration
    // replace this deterministic single-module build without touching users.
    let banner = "// Generated from kernel/src/index.ts. Do not edit.\n";
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("kernel.mjs");
    fs::write(output, format!("{banner}{code}")).expect("write built kernel");
}
