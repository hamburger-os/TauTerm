/**
 * 版本同步脚本
 * 从 package.json 读取规范版本号，同步写入 Tauri/Cargo 版本元数据。
 * package-lock.json 由 npm version 自身维护，Cargo.lock 中 TauTerm 自身版本由本脚本维护。
 *
 * 用法：
 *   node scripts/sync-version.js          # 手动同步
 *   npm version patch|minor|major         # bump 后自动触发（通过 postversion hook）
 *   npm version X.Y.Z --no-git-tag-version
 */

import { readFileSync, writeFileSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, "..");

function fail(message) {
  throw new Error(message);
}

// 1. 读取规范版本号（单一真相源）
const pkgPath = resolve(root, "package.json");
const pkg = JSON.parse(readFileSync(pkgPath, "utf-8"));
const version = pkg.version;

if (!/^\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?$/.test(version)) {
  fail(`package.json contains unsupported version: ${version}`);
}

console.log(`📦 版本号：${version}`);

// 2. 同步到 tauri.conf.json
const tauriConfPath = resolve(root, "src-tauri", "tauri.conf.json");
const tauriConf = JSON.parse(readFileSync(tauriConfPath, "utf-8"));
const oldTauriVer = tauriConf.version;
tauriConf.version = version;
writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + "\n");
console.log(`  ✅ tauri.conf.json: ${oldTauriVer} → ${version}`);

// 3. 同步到 Cargo.toml（仅在 [package] 段内替换第一条 version）
const cargoPath = resolve(root, "src-tauri", "Cargo.toml");
let cargoContent = readFileSync(cargoPath, "utf-8");
let oldCargoVer = null;
cargoContent = cargoContent.replace(
  /^(\[package\][\s\S]*?)^version\s*=\s*"([^"]+)"/m,
  (match, prefix, oldVer) => {
    oldCargoVer = oldVer;
    return `${prefix}version = "${version}"`;
  },
);

if (!oldCargoVer) {
  fail("Cargo.toml: 未找到 [package] 段的 version 字段");
}
writeFileSync(cargoPath, cargoContent);
console.log(`  ✅ Cargo.toml:      ${oldCargoVer} → ${version}`);

// 4. 同步 Cargo.lock 中工作区根包自身版本。
// npm version 会维护 package-lock.json；Cargo 不会被 npm 触发，因此这里显式同步，
// 保证随后所有 --locked CI/Release 命令都能直接运行。
const cargoLockPath = resolve(root, "src-tauri", "Cargo.lock");
let cargoLockContent = readFileSync(cargoLockPath, "utf-8");
let oldCargoLockVer = null;
cargoLockContent = cargoLockContent.replace(
  /^(\[\[package\]\]\s*\nname\s*=\s*"tauterm"\s*\nversion\s*=\s*")([^"]+)(")/m,
  (match, prefix, oldVer, suffix) => {
    oldCargoLockVer = oldVer;
    return `${prefix}${version}${suffix}`;
  },
);

if (!oldCargoLockVer) {
  fail("Cargo.lock: 未找到 tauterm 根包版本");
}
writeFileSync(cargoLockPath, cargoLockContent);
console.log(`  ✅ Cargo.lock:      ${oldCargoLockVer} → ${version}`);

console.log(`🎉 版本同步完成（${version}）`);
