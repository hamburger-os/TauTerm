/**
 * apply-logo.mjs — 一键将 logo.png 部署到所有需要的位置
 *
 * 用法: node scripts/apply-logo.mjs
 *
 * 将 src/assets/icons/logo.png：
 *   1. 复制到 src-tauri/icons/icon.png（窗口/任务栏图标）
 *   2. 生成 src-tauri/icons/icon.ico（Windows 编译要求，PNG 包裹在 ICO 容器中）
 *
 * 运行前请确保已将 AI 生成的 logo.png 放入 src/assets/icons/ 目录。
 */

import { readFileSync, writeFileSync, copyFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "..");

const LOGO_SRC = resolve(ROOT, "src/assets/icons/logo.png");
const ICON_PNG = resolve(ROOT, "src-tauri/icons/icon.png");
const ICON_ICO = resolve(ROOT, "src-tauri/icons/icon.ico");

// ── 检查源文件 ──────────────────────────────────────────────
if (!existsSync(LOGO_SRC)) {
  console.error("错误: 找不到 src/assets/icons/logo.png");
  console.error("请先将 AI 生成的 logo.png 放入该目录，再运行此脚本。");
  process.exit(1);
}

const pngBuffer = readFileSync(LOGO_SRC);
console.log(`读取: ${LOGO_SRC} (${pngBuffer.length} bytes)`);

// ── 1. 复制 logo.png → icon.png ────────────────────────────
copyFileSync(LOGO_SRC, ICON_PNG);
console.log(`复制: ${LOGO_SRC} → ${ICON_PNG}`);

// ── 2. 生成 icon.ico（PNG 包裹在 ICO 容器中） ────────────────
// ICO 格式参考: https://en.wikipedia.org/wiki/ICO_(file_format)
// 现代 ICO 可直接嵌入 PNG 数据，Windows Vista+ 均支持

const pngSize = pngBuffer.length;
const dataOffset = 6 + 16; // ICO header + 1 directory entry

// ICO header: reserved(2) + type 1=ICO(2) + image count(2)
const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0);     // reserved
header.writeUInt16LE(1, 2);     // type: ICO
header.writeUInt16LE(1, 4);     // image count

// Directory entry for a single PNG-embedded icon
const entry = Buffer.alloc(16);
const imgSize = Math.min(pngSize, 256);
const dim = imgSize >= 256 ? 0 : imgSize; // ICO uses 0 to represent 256
entry.writeUInt8(dim, 0); // width
entry.writeUInt8(dim, 1); // height
entry.writeUInt8(0, 2);               // color palette count
entry.writeUInt8(0, 3);               // reserved
entry.writeUInt16LE(1, 4);            // color planes
entry.writeUInt16LE(32, 6);           // bits per pixel
entry.writeUInt32LE(pngSize, 8);      // image size (raw PNG)
entry.writeUInt32LE(dataOffset, 12);   // offset to PNG data

const ico = Buffer.concat([header, entry, pngBuffer]);
writeFileSync(ICON_ICO, ico);
console.log(`生成: ${ICON_ICO} (${ico.length} bytes)`);

console.log("\n完成! logo.png 已部署到所有位置。");
console.log("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
console.log("📌 Windows 任务栏图标更新提示：");
console.log("  1. 重新构建应用: cargo tauri build (或 cargo tauri dev)");
console.log("  2. 清除图标缓存: ie4uinit.exe -show");
console.log("  3. 如仍未更新，从任务栏取消固定后重新固定应用");
console.log("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
