/**
 * Windows 子进程可见性契约。
 *
 * Release 构建是 GUI 子系统（`src-tauri/src/main.rs` 的 `windows_subsystem = "windows"`），
 * 进程自身没有控制台。于是任何 console 子系统子进程都会被 Windows 新分配一个控制台
 * 窗口，表现为用户可见的“黑框一闪而过”。后台探针、探测、助手进程必须经
 * `kernel::process::hidden_command` 启动（`CREATE_NO_WINDOW`）。
 *
 * 只有需要用户真正看见的程序（系统文件管理器等）可以继续用裸 `Command::new`，
 * 且必须在同一行标注 `// visible-launcher: <理由>`，让例外在评审里可见。
 */
import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve("src-tauri/src");
const HELPER = path.join(ROOT, "kernel", "process.rs");
const VISIBLE_LAUNCHER = /\/\/\s*visible-launcher:\s*\S/;
const RAW_COMMAND = /\bCommand::new\s*\(/;

function rustFiles(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return rustFiles(full);
    return entry.name.endsWith(".rs") ? [full] : [];
  });
}

const violations = [];

const files = rustFiles(ROOT);
if (!files.includes(HELPER)) {
  violations.push("src-tauri/src/kernel/process.rs is missing (hidden_command owner)");
} else {
  const helper = fs.readFileSync(HELPER, "utf8");
  if (!helper.includes("pub fn hidden_command(")) {
    violations.push("src-tauri/src/kernel/process.rs must own `pub fn hidden_command(`");
  }
  if (!helper.includes("CREATE_NO_WINDOW")) {
    violations.push("src-tauri/src/kernel/process.rs must apply CREATE_NO_WINDOW");
  }
}

for (const file of files) {
  if (file === HELPER) continue;
  const lines = fs.readFileSync(file, "utf8").split("\n");
  lines.forEach((line, index) => {
    if (!RAW_COMMAND.test(line)) return;
    if (VISIBLE_LAUNCHER.test(line)) return;
    violations.push(
      path.relative(".", file).replaceAll(path.sep, "/") +
        ":" +
        (index + 1) +
        " spawns a console child with raw `Command::new`; use kernel::process::hidden_command " +
        "(or annotate `// visible-launcher: <reason>` when the user must see the window)",
    );
  });
}

if (violations.length) {
  console.error("Windows console-spawn contract failed:");
  for (const violation of violations) console.error("  - " + violation);
  process.exit(1);
}
console.log(
  `windows-spawn: ${files.length} Rust sources route console children through hidden_command`,
);
