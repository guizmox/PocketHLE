fn main() {
    println!("cargo:rerun-if-changed=assets/pockethle.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winres::WindowsResource::new()
            .set_icon("assets/pockethle.ico")
            .set("ProductName", "PocketHLE-Reborn")
            .set("FileDescription", "PocketHLE-Reborn")
            .compile()
            .expect("embedding the PocketHLE-Reborn Windows icon");
    }
}
