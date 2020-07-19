use cmake::Config;
use std::env;
use std::path::Path;

fn main() {
    let hw_dir_path = env::current_dir().unwrap().join(Path::new("hw"));
    let dst = Config::new("third_party/protobridge")
        .define("PROTOBRIDGE_HW_PATH", hw_dir_path)
        .define("PROTOBRIDGE_TRACE", "ON")
        .build();
    println!(
        "cargo:rustc-link-search=native={}",
        format!("{}/lib", dst.display())
    );
    println!("cargo:rustc-link-lib=static=protobridge");
}
