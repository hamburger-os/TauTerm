/**
 * 版本同步脚本
 * 从 package.json 读取版本号，同步写入 tauri.conf.json 和 Cargo.toml。
 *
 * 用法：
 *   node scripts/sync-version.js          # 手动同步
 *   npm version patch|minor|major         # bump 后自动触发（通过 postversion hook）
 */

import { readFileSync, writeFileSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, "..");

// 1. 读取规范版本号（单一真相源）
const pkgPath = resolve(root, "package.json");
const pkg = JSON.parse(readFileSync(pkgPath, "utf-8"));
const version = pkg.version;

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

// 匹配 [package] 段内的 version 字段（使用函数式替换以提取旧值）
let oldCargoVer = null;
cargoContent = cargoContent.replace(
  /^(\[package\][\s\S]*?)^version\s*=\s*"([^"]+)"/m,
  (match, prefix, oldVer) => {
    oldCargoVer = oldVer;
    return `${prefix}version = "${version}"`;
  }
);

if (oldCargoVer) {
  writeFileSync(cargoPath, cargoContent);
  console.log(`  ✅ Cargo.toml:      ${oldCargoVer} → ${version}`);
} else {
  console.warn(`  ⚠️  Cargo.toml: 未找到 [package] 段的 version 字段`);
}

console.log(`🎉 版本同步完成（${version}）`);
