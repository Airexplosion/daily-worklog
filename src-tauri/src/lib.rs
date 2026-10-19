pub mod model;
pub mod storage;
pub mod format;
pub mod ai;
pub mod tags;
pub mod holidays;

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("启动失败");
}
