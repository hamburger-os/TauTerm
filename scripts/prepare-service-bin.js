/**
 * 将 cargo 构建出的 tauterm-service.exe 复制到 bundle.resources 期望的位置。
 *
 * 在打包前（beforeBundleCommand，即 cargo build 之后）把
 * `target/release/tauterm-service.exe` 复制到 `src-tauri/binaries/tauterm-service.exe`，
 * 由 `tauri.conf.json` 的 `bundle.resources` 打包进安装器（落于 `$INSTDIR`），
 * 并在安装时被 NSIS hook 注册为 Windows 服务。
 */

import { copyFileSync, mkdirSync, existsSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, '..');

// 非 Windows 平台无需服务二进制
if (process.platform !== 'win32') {
  console.log('⏭  Skipped (non-Windows platform — service is Windows-only)');
  process.exit(0);
}

const src = join(root, 'src-tauri', 'target', 'release', 'tauterm-service.exe');
const binDir = join(root, 'src-tauri', 'binaries');
const dst = join(binDir, 'tauterm-service.exe');

if (!existsSync(src)) {
  console.error(`❌ ERROR: service binary not found: ${src}`);
  console.error('     Ensure `cargo build --release` produced tauterm-service.exe first.');
  process.exit(1);
}

mkdirSync(binDir, { recursive: true });
copyFileSync(src, dst);
console.log(`✅ Copied service binary -> ${dst}`);
