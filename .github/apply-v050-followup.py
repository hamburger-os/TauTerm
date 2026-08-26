from pathlib import Path


def replace(path: str, old: str, new: str, expected: int = 1) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != expected:
        raise SystemExit(
            f"{path}: expected {expected} occurrence(s), found {count}: {old!r}"
        )
    p.write_text(text.replace(old, new), encoding="utf-8")


replace(
    "src-tauri/src/channel/async_io_loop.rs",
    "use std::sync::{mpsc, Arc};",
    "use std::sync::Arc;",
)
replace(
    "src-tauri/src/plugins/network/mod.rs",
    "use crate::channel::io_loop::{IoLoopCmd, IoLoopContext};",
    "use crate::channel::io_loop::IoLoopCmd;",
)
replace(
    "src-tauri/src/kernel/session_store.rs",
    "            plugin_id: plugin_id,",
    "            plugin_id,",
    2,
)
replace(
    "src-tauri/src/kernel/session_store.rs",
    "            endpoint: endpoint,",
    "            endpoint,",
    2,
)
replace(
    "src-tauri/src/transfer/serial_transfer.rs",
    "aggregate_total: aggregate_total,",
    "aggregate_total,",
    2,
)
replace(
    "src-tauri/src/transfer/zmodem.rs",
    r'b"test.txt\000100 1234567890 100644 0 100"',
    r'b"test.txt\x0000100 1234567890 100644 0 100"',
)
replace(
    "src-tauri/src/transfer/zmodem.rs",
    r'b"test.txt\000100 1234567890"',
    r'b"test.txt\x0000100 1234567890"',
)
replace(
    "src-tauri/src/plugins/iperf/server.rs",
    "    if let Ok(mut h) = server_handle.lock() {\n        *h = Some(handle);\n    }\n}",
    "    if let Ok(mut h) = server_handle.lock() {\n        *h = Some(handle);\n    };\n}",
)

Path("src-tauri/build.rs").write_text(
    '''fn main() {
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
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH")
            .expect("CARGO_CFG_TARGET_ARCH is unavailable");
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
''',
    encoding="utf-8",
)

vite = Path("vite.config.ts")
vite_text = vite.read_text(encoding="utf-8")
old = '''  build: {
    // xterm.js + framer-motion 总体积超过默认 500KB 限制，提高阈值
    chunkSizeWarningLimit: 800,
  },'''
new = '''  build: {
    rollupOptions: {
      output: {
        // Split the largest runtime families instead of hiding Vite's chunk-size warning.
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (id.includes("@xterm/")) return "xterm";
          if (id.includes("framer-motion") || id.includes("motion-dom") || id.includes("motion-utils")) return "motion";
          if (id.includes("react") || id.includes("scheduler")) return "react";
          if (id.includes("i18next")) return "i18n";
          if (id.includes("@tauri-apps/")) return "tauri";
          return "vendor";
        },
      },
    },
  },'''
if vite_text.count(old) != 1:
    raise SystemExit("vite.config.ts: expected legacy chunkSizeWarningLimit block once")
vite.write_text(vite_text.replace(old, new), encoding="utf-8")
