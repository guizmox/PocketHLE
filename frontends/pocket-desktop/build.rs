fn main() {
    println!("cargo:rerun-if-changed=assets/pockethle.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let exports = [
            "NvOptimusEnablement",
            "AmdPowerXpressRequestHighPerformance",
        ];
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            for symbol in exports {
                println!("cargo:rustc-link-arg-bins=/EXPORT:{symbol},DATA");
            }
        } else {
            let definition = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
                .join("gpu-exports.def");
            std::fs::write(
                &definition,
                format!(
                    "EXPORTS\n{}\n",
                    exports.map(|s| format!("{s} DATA")).join("\n")
                ),
            )
            .unwrap();
            println!("cargo:rustc-link-arg-bins={}", definition.display());
        }
        winres::WindowsResource::new()
            .set_icon("assets/pockethle.ico")
            .set("ProductName", "PocketHLE")
            .set("FileDescription", "PocketHLE")
            .compile()
            .expect("embedding the PocketHLE Windows icon");
    }
}
