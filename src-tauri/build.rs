fn main() {
    // 服务二进制 tauterm-service.exe 由 cargo 在同一构建中产出，但 tauri-build
    // 会在构建脚本阶段校验 bundle.resources 路径必须存在。这里先创建占位文件，
    // 真正的二进制由 scripts/prepare-service-bin.js 在 beforeBundleCommand
    // （cargo build 之后）覆盖为实际产物。
    #[cfg(target_os = "windows")]
    {
        let _ = std::fs::create_dir_all("binaries");
        let placeholder = std::path::Path::new("binaries").join("tauterm-service.exe");
        if !placeholder.exists() {
            let _ = std::fs::write(&placeholder, b"placeholder");
        }
    }

    tauri_build::build();

    // com0com 驱动文件仅在 Windows 上存在和需要
    #[cfg(target_os = "windows")]
    {
        // 根据目标架构将对应的 com0com 驱动文件复制到 resources/com0com/ 根目录
        // Tauri bundle resources 将从此处打包
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        let arch_dir = if arch == "x86_64" {
            "x64"
        } else if arch == "x86" {
            "x86"
        } else {
            println!(
                "cargo:warning=com0com: unsupported target architecture '{}', \
                 skipping driver file copy (virtual port feature will be unavailable)",
                arch
            );
            return;
        };

        let base = std::path::Path::new("../resources/com0com");
        let src_dir = base.join(arch_dir);
        let dst_dir = base;

        let required_files = ["setupc.exe", "setup.dll", "com0com.sys", "com0com.inf", "com0com.cat", "cncport.inf", "comport.inf"];
        let mut all_ok = true;

        for file in &required_files {
            let src = src_dir.join(file);
            let dst = dst_dir.join(file);
            if src.exists() {
                if let Err(e) = std::fs::copy(&src, &dst) {
                    println!("cargo:warning=com0com: failed to copy {}/{}: {}", arch_dir, file, e);
                    all_ok = false;
                }
            } else {
                println!("cargo:warning=com0com: {}/{} not found", arch_dir, file);
                all_ok = false;
            }
        }

        if !all_ok {
            println!("cargo:warning=com0com: some driver files missing, virtual serial port feature may be unavailable");
        } else {
            println!("cargo:warning=com0com: {} driver files copied successfully", arch_dir);
        }
    }

    #[cfg(not(target_os = "windows"))]
    println!("cargo:warning=com0com: skipped (non-Windows target)");
}
