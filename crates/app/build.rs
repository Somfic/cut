use std::process::Command;

fn main() {
    // generate types
    draad_codegen::Config::new()
        .root(env!("CARGO_MANIFEST_DIR"))
        .generated_rs("src/generated.rs")
        .client_dir("../../ui/src/lib/schema")
        .generate()
        .expect("draad codegen");

    // frontend
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        build_frontend();
    } else {
        use_dev_server();
    }

    // build tauri stuff
    tauri_build::build()
}

const UI_DIR: &str = "../../ui";
const DEV_URL: &str = "http://localhost:5173";

fn use_dev_server() {
    let patch = format!(r#"{{"build":{{"devUrl":"{DEV_URL}"}}}}"#);

    //  read by `tauri_build::build()` in this process
    unsafe { std::env::set_var("TAURI_CONFIG", &patch) };

    // read by `generate_context!` when rustc expands it
    println!("cargo:rustc-env=TAURI_CONFIG={patch}");
}

fn build_frontend() {
    println!("cargo:rerun-if-changed={UI_DIR}/src");
    println!("cargo:rerun-if-changed={UI_DIR}/package.json");

    // build frontend
    let status = Command::new("bun")
        .args(["run", "build"])
        .current_dir(UI_DIR)
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("frontend build failed ({s})"),
        Err(e) => panic!("could not run `bun run build` in {UI_DIR}: {e}"),
    }
}
