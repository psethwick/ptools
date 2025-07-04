use std::path::PathBuf;

pub fn get_data_path(name: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|mut path| {
        path.push("yoink");
        path.push(name);
        path
    })
}
