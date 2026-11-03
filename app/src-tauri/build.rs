fn main() {
    println!("cargo:rerun-if-env-changed=PROJECT_O_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=PROJECT_O_BUILD_TIME");
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
