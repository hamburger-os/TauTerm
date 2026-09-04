fn main() {
    // Platform bundle configs declare the TRDP helper as a resource. The real
    // executable is built by scripts/prepare-service-bin.js in beforeBundleCommand,
    // which runs after the Rust build. Create a placeholder first so tauri-build
    // can validate bundle resources during the build-script phase.
    {
        std::fs::create_dir_all("binaries")
            .expect("failed to create src-tauri/binaries for TRDP helper");
        let helper_name = if cfg!(target_os = "windows") {
            "tauterm-trdp-bridge.exe"
        } else {
            "tauterm-trdp-bridge"
        };
        let placeholder = std::path::Path::new("binaries").join(helper_name);
        if !placeholder.exists() {
            std::fs::write(&placeholder, b"placeholder")
                .expect("failed to create TRDP bridge placeholder");
        }
    }

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
        // 此段代码对每次 Windows cargo build（含 dev/clippy/test）都会执行，
        // 不只是在 tauri 打包时。因此缺失驱动文件或非 x86/x64 目标只降级为警告：
        // 让普通开发构建与交叉编译能继续，而把“产物必须完整”的强校验交给
        // release 工作流的资产校验步骤（fail-closed 在那里兜底）。
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        let arch_dir = match arch.as_str() {
            "x86_64" => "x64",
            "x86" => "x86",
            other => {
                println!(
                    "cargo:warning=com0com: unsupported target architecture '{other}', \
                     skipping driver copy (virtual port feature will be unavailable)"
                );
                return;
            }
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

        let mut all_ok = true;
        for file in required_files {
            let src = src_dir.join(file);
            let dst = base.join(file);
            if src.exists() {
                if let Err(error) = std::fs::copy(&src, &dst) {
                    println!("cargo:warning=com0com: failed to copy {arch_dir}/{file}: {error}");
                    all_ok = false;
                }
            } else {
                println!("cargo:warning=com0com: {arch_dir}/{file} not found");
                all_ok = false;
            }
        }

        if !all_ok {
            println!(
                "cargo:warning=com0com: some driver files missing, virtual serial port feature may be unavailable"
            );
        }
    }
}
