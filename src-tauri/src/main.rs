// يمنع ظهور نافذة سوداء (كونسول) مع التطبيق على ويندوز
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    kemet_download_lib::run()
}
