fn main() {
    // Ensure cargo rebuilds when config.toml changes
    // This is needed because include_str!() doesn't automatically track file dependencies
    println!("cargo::rerun-if-changed=config.toml");
}
