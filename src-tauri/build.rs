fn main() {
    tauri_build::build();

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

    // Windows 管理员权限清单由 scripts/set-admin-manifest.js 在构建后通过 mt.exe 嵌入
    // 原因: Tauri 内部已生成 Windows 资源（含 VERSION + asInvoker manifest），
    // 无法在 build.rs 中叠加第二个资源。mt.exe 可直接替换 PE 中已有的 manifest。
}
