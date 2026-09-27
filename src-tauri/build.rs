fn main() {
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("native/macos.m")
            .flag("-fobjc-arc")
            .compile("clipvault_macos");
        for framework in ["AppKit", "ApplicationServices", "Security", "ImageIO"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        println!("cargo:rerun-if-changed=native/macos.m");
    }
    tauri_build::build()
}
