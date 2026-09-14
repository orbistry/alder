use std::{env, fs, path::PathBuf};

fn main() {
    let source = PathBuf::from("kernel/src/index.ts");
    println!("cargo::rerun-if-changed={}", source.display());
    let code = fs::read_to_string(&source).expect("read kernel TypeScript");
    let web_source = PathBuf::from("kernel/src/web.ts");
    println!("cargo::rerun-if-changed={}", web_source.display());
    let web = fs::read_to_string(web_source).expect("read web kernel TypeScript");
    let transport_source = PathBuf::from("kernel/src/transport.ts");
    println!("cargo::rerun-if-changed={}", transport_source.display());
    let transport = fs::read_to_string(transport_source).expect("read transport kernel TypeScript");
    let application_source = PathBuf::from("kernel/src/application.ts");
    println!("cargo::rerun-if-changed={}", application_source.display());
    let application =
        fs::read_to_string(application_source).expect("read application kernel TypeScript");
    let http_source = PathBuf::from("kernel/src/http.ts");
    println!("cargo::rerun-if-changed={}", http_source.display());
    let http = fs::read_to_string(http_source).expect("read HTTP kernel TypeScript");
    let cloudflare_source = PathBuf::from("kernel/src/cloudflare.ts");
    println!("cargo::rerun-if-changed={}", cloudflare_source.display());
    let cloudflare =
        fs::read_to_string(cloudflare_source).expect("read Cloudflare kernel TypeScript");
    let wire_source = PathBuf::from("kernel/src/wire_validate.ts");
    println!("cargo::rerun-if-changed={}", wire_source.display());
    let wire = fs::read_to_string(wire_source).expect("read remote wire validator TypeScript");
    let forms_source = PathBuf::from("kernel/src/forms.ts");
    println!("cargo::rerun-if-changed={}", forms_source.display());
    let forms = fs::read_to_string(forms_source).expect("read forms kernel TypeScript");
    // The M2 kernel intentionally uses the JavaScript subset of TypeScript.
    // Keeping this boundary in one build script lets the rolldown integration
    // replace this deterministic single-module build without touching users.
    let banner = "// Generated from kernel/src/index.ts. Do not edit.\n";
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("kernel.mjs");
    fs::write(
        output,
        format!("{banner}{code}\n{web}\n{transport}\n{application}\n{http}\n{cloudflare}\n{wire}\n{forms}"),
    )
    .expect("write built kernel");
}
