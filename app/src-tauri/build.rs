fn main() {
    println!("cargo:rerun-if-env-changed=PROJECT_O_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=PROJECT_O_BUILD_TIME");
    tauri_build::build()
}
