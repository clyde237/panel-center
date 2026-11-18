fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let inventory = panel_center_lib::services::inventory::collect_with_paths(&paths);
    println!("{}", serde_json::to_string_pretty(&inventory).unwrap());
}
