fn main() {
    // The service binary is produced by Cargo in the same build, while tauri-build
    // validates bundle.resources during the build-script phase. Create a placeholder
    // first; scripts/prepare-service-bin.js replaces it with the real binary before bundling.
    #[cfg(target_os = "windows")]
    {
        std::fs::create_dir_all("binaries")
            .expect("failed to create src-tauri/binaries for tauterm-service");
        let placeholder = std::path::Path::new("binaries").join("tauterm-service.exe");
        if !placeholder.exists() {
            std::fs::write(&placeholder, b"placeholder")
                .expect("failed to create tauterm-service.exe placeholder");
        }
    }

    tauri_build::build();

    #[cfg(target_os = "windows")]
    {
        let arch =
            std::env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH is unavailable");
        let arch_dir = match arch.as_str() {
            "x86_64" => "x64",
            "x86" => "x86",
            other => panic!(
                "com0com does not support target architecture {other}; refusing to build a package without required driver resources"
            ),
        };

        let base = std::path::Path::new("../resources/com0com");
        let src_dir = base.join(arch_dir);
        let required_files = [
            "setupc.exe",
            "setup.dll",
            "com0com.sys",
            "com0com.inf",
            "com0com.cat",
            "cncport.inf",
            "comport.inf",
        ];

        for file in required_files {
            let src = src_dir.join(file);
            let dst = base.join(file);
            assert!(
                src.exists(),
                "required com0com resource is missing: {}",
                src.display()
            );
            std::fs::copy(&src, &dst).unwrap_or_else(|error| {
                panic!(
                    "failed to copy required com0com resource {} -> {}: {error}",
                    src.display(),
                    dst.display()
                )
            });
        }
    }
}
