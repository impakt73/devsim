use cmake::Config;
use std::env;
use std::path::Path;

fn main() {
    let hw_dir_path = env::current_dir().unwrap().join(Path::new("hw"));
    let mut config = Config::new("third_party/protobridge");

    config.define("PROTOBRIDGE_HW_PATH", hw_dir_path);

    // Only enable VCD traces in debug builds
    if cfg!(debug_assertions) {
        config.define("PROTOBRIDGE_TRACE", "ON");
    }

    let dst = config.build();

    println!(
        "cargo:rustc-link-search=native={}",
        format!("{}/lib", dst.display())
    );
    println!("cargo:rustc-link-lib=static=protobridge");
}
