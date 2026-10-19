pub mod model;
pub mod storage;

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("启动失败");
}
