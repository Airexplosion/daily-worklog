fn main() {
    // 更新检查的仓库与发布人名字在构建时注入，见 src/update.rs
    println!("cargo:rerun-if-env-changed=DWL_UPDATE_REPO");
    println!("cargo:rerun-if-env-changed=DWL_PUBLISHER");
    tauri_build::build()
}
