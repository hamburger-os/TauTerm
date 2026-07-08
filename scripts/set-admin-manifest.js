/**
 * 在 Tauri 构建后将 requireAdministrator 清单嵌入到 Windows 可执行文件中。
 *
 * 通过 mt.exe (Windows SDK Manifest Tool) 替换 PE 文件中已有的 manifest，
 * 将 Tauri 默认的 asInvoker 执行级别改为 requireAdministrator。
 *
 * 原因: Tauri build.rs 内部生成的 Windows 资源已包含 VERSION + manifest，
 * 无法通过 winresource/embed-resource 叠加第二个资源（CVT1100 重复错误）。
 * mt.exe 直接操作 PE 文件，完美绕过此限制。
 *
 * 由 tauri.conf.json build.beforeBundleCommand 调用（在 cargo build 之后、NSIS 打包之前）。
 */

import { execSync } from 'child_process';
import { existsSync, readdirSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = join(__dirname, '..');

const manifestPath = join(rootDir, 'src-tauri', 'windows', 'manifest.xml');
const exePath = join(rootDir, 'src-tauri', 'target', 'release', 'tauterm.exe');

console.log('--- TauTerm: Embedding Administrator Manifest ---');

// 非 Windows 平台跳过
if (process.platform !== 'win32') {
  console.log('  ⏭  Skipped (non-Windows platform)');
  process.exit(0);
}

if (!existsSync(exePath)) {
  console.error(`  ❌ ERROR: Executable not found: ${exePath}`);
  console.error('     Make sure the Rust binary has been built first.');
  process.exit(1);
}

if (!existsSync(manifestPath)) {
  console.error(`  ❌ ERROR: Manifest not found: ${manifestPath}`);
  process.exit(1);
}

/**
 * 尝试运行 mt.exe 嵌入 manifest。
 * @param {string} mtExePath - mt.exe 的完整路径或命令名
 * @returns {boolean} 是否成功
 */
function tryEmbed(mtExePath) {
  const cmd = `"${mtExePath}" -manifest "${manifestPath}" -outputresource:"${exePath}";1`;
  console.log(`  > ${cmd}`);
  try {
    execSync(cmd, { stdio: 'inherit' });
    return true;
  } catch {
    return false;
  }
}

// 方法 1: 先检查 PATH 中是否有 mt.exe（where 命令静默检查，不输出错误）
let mtInPath = false;
try {
  execSync('where mt.exe', { stdio: 'pipe' });
  mtInPath = true;
} catch {
  mtInPath = false;
}

if (mtInPath && tryEmbed('mt.exe')) {
  console.log('  ✅ Administrator manifest embedded successfully.\n');
  process.exit(0);
}

// 方法 2: 在 Windows SDK 常见路径中搜索 mt.exe
if (!mtInPath) {
  console.log('  ℹ  mt.exe not in PATH, searching Windows SDK directories...');
}

const kitDir = 'C:\\Program Files (x86)\\Windows Kits\\10\\bin';
if (existsSync(kitDir)) {
  try {
    const entries = readdirSync(kitDir);
    const versionDirs = entries
      .filter(e => /^\d+\.\d+\.\d+/.test(e))
      .sort()
      .reverse();

    for (const versionDir of versionDirs) {
      const mtPath = join(kitDir, versionDir, 'x64', 'mt.exe');
      if (existsSync(mtPath) && tryEmbed(mtPath)) {
        console.log('  ✅ Administrator manifest embedded successfully.\n');
        process.exit(0);
      }
    }
  } catch {
    // 读取目录失败，继续报错
  }
}

console.warn('  ⚠ WARNING: mt.exe not found. Install Windows SDK to embed admin manifest.');
console.warn('     Manifest was NOT embedded. The app will run without admin privileges.');
console.warn('     Production builds should have Windows SDK installed.');
process.exit(0);
