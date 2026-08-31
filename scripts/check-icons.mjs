#!/usr/bin/env node

/**
 * Validates the committed PNG icon set without relying on a native image tool.
 * `npm run check:icons -- --strict` promotes optical-frame and hue warnings to
 * failures after an asset refresh has been approved.
 */
import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { inflateSync } from "node:zlib";
import { resolve } from "node:path";

const ICON_DIR = resolve("src/assets/icons");
const STRICT = process.argv.includes("--strict");
const EXPECTED = [
  "appearance", "arrow-down", "arrow-left", "arrow-right", "arrow-up", "caret-down",
  "chart", "check", "check-circle", "chevron-down", "chevron-right", "chevron-up",
  "clipboard", "close", "code", "commands", "connection", "construction", "download",
  "drag-handle", "edit", "endpoint", "file", "folder", "globe", "hourglass", "info",
  "keyboard", "lock", "log", "logo", "loop", "package", "paste", "play", "plus",
  "refresh", "robot", "search", "send", "settings", "sidebar-left", "sidebar-right",
  "ssh-shell", "status-cancelled", "status-skipped", "steps", "stop", "stopwatch", "tag",
  "transfer-active", "trash", "upload", "view-grid", "view-list", "warning", "window-close",
  "window-maximize", "window-minimize", "window-restore", "x-circle",
].sort();

// These recently regenerated assets are used at 12–18px and need a strong
// alpha core, not merely a large transparent canvas.  Keep the minimums
// explicit so a visually undersized replacement cannot pass by accident.
// `window-minimize` is intentionally exempt: its short horizontal stroke is
// the complete semantic shape and must not be rejected for its height.
const CORE_BBOX_RULES = new Map([
  ["clipboard", { minWidth: 140, minHeight: 120, minLongEdge: 176 }],
  ["commands", { minWidth: 145, minHeight: 105, minLongEdge: 176 }],
  ["log", { minWidth: 150, minHeight: 110, minLongEdge: 176 }],
  // Paste is intentionally portrait-oriented; its 184px-tall board carries
  // the visual weight, while the slightly wider body keeps it aligned with
  // clipboard without requiring an exact pixel width.
  ["paste", { minWidth: 145, minHeight: 120, minLongEdge: 176 }],
  ["ssh-shell", { minWidth: 150, minHeight: 110, minLongEdge: 176 }],
  ["status-cancelled", { minWidth: 150, minHeight: 150, minLongEdge: 176 }],
  ["steps", { minWidth: 150, minHeight: 80, minLongEdge: 176 }],
  ["transfer-active", { minWidth: 176, minHeight: 64, minLongEdge: 176 }],
  // Send is a compact data slot plus a short arrow; reject panorama-like
  // replacements while allowing normal anti-aliased shape variation.
  ["send", { minWidth: 145, minHeight: 110, minLongEdge: 165, maxAspectRatio: 1.75 }],
  ["package", { minWidth: 140, minHeight: 120, minLongEdge: 176 }],
  // Both sidebars use the same near-square frame.  The lower bound is broad
  // enough for optical padding, while the aspect cap prevents long strips.
  ["sidebar-left", { minWidth: 170, minHeight: 142, minLongEdge: 176, maxAspectRatio: 1.35 }],
  ["sidebar-right", { minWidth: 170, minHeight: 142, minLongEdge: 176, maxAspectRatio: 1.35 }],
  // A close X should occupy a square control-sized area, not pass as a tiny
  // diagonal mark.  The aspect cap is a geometry check, not a stroke-width
  // guess, so it remains stable across generated anti-aliasing differences.
  ["window-close", { minWidth: 150, minHeight: 150, minLongEdge: 174, maxAspectRatio: 1.3 }],
  ["window-maximize", { minWidth: 140, minHeight: 140, minLongEdge: 176 }],
  ["window-restore", { minWidth: 140, minHeight: 140, minLongEdge: 176 }],
  // Download remains a vertical control; its arrow should not collapse into
  // a short wide mark.  Exact arrow-head width is intentionally left to
  // visual review because the pixel checker cannot isolate that sub-shape.
  ["download", { minWidth: 90, minHeight: 176, minLongEdge: 176, minCorePixels: 16000, maxAspectRatio: 1.35 }],
]);

const signature = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
let errors = 0;
let warnings = 0;
const fail = (message) => { errors += 1; console.error(`ERROR: ${message}`); };
const warn = (message) => { warnings += 1; console.warn(`WARN: ${message}`); };

// Keep this list in lockstep with the component's single runtime registry.
// Parsing the map here makes drift fail loudly while retaining a plain list
// that is easy to review in this asset check.
const iconSource = readFileSync(resolve("src/components/common/Icon.tsx"), "utf8");
const mapBody = iconSource.match(/const PNG_MAP = \{([\s\S]*?)\n\} satisfies/);
if (!mapBody) throw new Error("Unable to locate PNG_MAP in Icon.tsx");
const REGISTERED = [...mapBody[1].matchAll(/(?:"([^"]+)"|([\w-]+))\s*:/g)]
  .map((match) => match[1] || match[2]).sort();
if (JSON.stringify(REGISTERED) !== JSON.stringify(EXPECTED)) {
  fail("EXPECTED icon list is out of sync with PNG_MAP in Icon.tsx");
  console.error(`  EXPECTED: ${EXPECTED.join(", ")}`);
  console.error(`  PNG_MAP:  ${REGISTERED.join(", ")}`);
}

function pngPixels(file) {
  const data = readFileSync(file);
  if (!data.subarray(0, 8).equals(signature)) throw new Error("not a PNG");
  let offset = 8;
  let width;
  let height;
  let depth;
  let colorType;
  const idat = [];
  while (offset < data.length) {
    const length = data.readUInt32BE(offset);
    const type = data.subarray(offset + 4, offset + 8).toString("ascii");
    const chunk = data.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = chunk.readUInt32BE(0);
      height = chunk.readUInt32BE(4);
      depth = chunk[8];
      colorType = chunk[9];
    } else if (type === "IDAT") idat.push(chunk);
    offset += length + 12;
  }
  if (depth !== 8 || colorType !== 6) throw new Error(`requires 8-bit RGBA, got depth=${depth}, type=${colorType}`);
  const packed = inflateSync(Buffer.concat(idat));
  const stride = width * 4;
  const pixels = Buffer.alloc(stride * height);
  let input = 0;
  for (let y = 0; y < height; y += 1) {
    const filter = packed[input++];
    const row = pixels.subarray(y * stride, (y + 1) * stride);
    const previous = y === 0 ? null : pixels.subarray((y - 1) * stride, y * stride);
    for (let x = 0; x < stride; x += 1) {
      const value = packed[input++];
      const left = x >= 4 ? row[x - 4] : 0;
      const up = previous ? previous[x] : 0;
      const upLeft = previous && x >= 4 ? previous[x - 4] : 0;
      if (filter === 0) row[x] = value;
      else if (filter === 1) row[x] = (value + left) & 255;
      else if (filter === 2) row[x] = (value + up) & 255;
      else if (filter === 3) row[x] = (value + Math.floor((left + up) / 2)) & 255;
      else if (filter === 4) {
        const p = left + up - upLeft;
        const pa = Math.abs(p - left), pb = Math.abs(p - up), pc = Math.abs(p - upLeft);
        row[x] = (value + (pa <= pb && pa <= pc ? left : pb <= pc ? up : upLeft)) & 255;
      } else throw new Error(`unsupported filter ${filter}`);
    }
  }
  return { width, height, pixels };
}

function inspect(name) {
  const file = resolve(ICON_DIR, `${name}.png`);
  const { width, height, pixels } = pngPixels(file);
  if (width !== 256 || height !== 256) fail(`${name}.png must be 256×256, got ${width}×${height}`);
  let minX = width, minY = height, maxX = -1, maxY = -1;
  let coreMinX = width, coreMinY = height, coreMaxX = -1, coreMaxY = -1;
  let alphaTotal = 0, red = 0, green = 0, blue = 0, corePixels = 0;
  for (let y = 0; y < height; y += 1) for (let x = 0; x < width; x += 1) {
    const i = (y * width + x) * 4;
    const alpha = pixels[i + 3];
    if (alpha <= 16) continue;
    minX = Math.min(minX, x); minY = Math.min(minY, y);
    maxX = Math.max(maxX, x); maxY = Math.max(maxY, y);
    alphaTotal += alpha;
    red += pixels[i] * alpha; green += pixels[i + 1] * alpha; blue += pixels[i + 2] * alpha;
    if (alpha >= 96) {
      corePixels += 1;
      coreMinX = Math.min(coreMinX, x); coreMinY = Math.min(coreMinY, y);
      coreMaxX = Math.max(coreMaxX, x); coreMaxY = Math.max(coreMaxY, y);
    }
  }
  if (maxX < 0) return fail(`${name}.png contains no visible pixels`);
  const coreRule = CORE_BBOX_RULES.get(name);
  if (coreRule && coreMaxX >= 0) {
    const coreWidth = coreMaxX - coreMinX + 1;
    const coreHeight = coreMaxY - coreMinY + 1;
    const coreLongEdge = Math.max(coreWidth, coreHeight);
    const coreAspectRatio = Math.max(coreWidth, coreHeight) / Math.min(coreWidth, coreHeight);
    if (coreWidth < coreRule.minWidth || coreHeight < coreRule.minHeight || coreLongEdge < coreRule.minLongEdge) {
      fail(`${name}.png alpha core is too small (${coreWidth}×${coreHeight}); requires at least ${coreRule.minWidth}×${coreRule.minHeight} and a ${coreRule.minLongEdge}px long edge`);
    }
    if (coreRule.maxAspectRatio && coreAspectRatio > coreRule.maxAspectRatio) {
      fail(`${name}.png alpha core is too elongated (${coreWidth}×${coreHeight}, aspect ${coreAspectRatio.toFixed(2)}); requires aspect ≤ ${coreRule.maxAspectRatio}`);
    }
    if (coreRule.minCorePixels && corePixels < coreRule.minCorePixels) {
      fail(`${name}.png alpha core is too light (${corePixels}px); requires at least ${coreRule.minCorePixels}px`);
    }
  } else if (coreRule) {
    fail(`${name}.png has no sufficiently opaque alpha core`);
  }
  if (minX === 0 || minY === 0 || maxX === width - 1 || maxY === height - 1) fail(`${name}.png has opaque pixels on the canvas edge`);
  const padding = Math.min(minX, minY, width - 1 - maxX, height - 1 - maxY);
  if (name !== "logo" && (minX < 32 || minY < 32 || maxX > 223 || maxY > 223)) warn(`${name}.png escapes the 192px optical frame [32,223] (padding ${padding}px)`);
  if (name !== "logo") {
    const digest = createHash("sha256").update(readFileSync(file)).digest("hex");
    const duplicate = seenHashes.get(digest);
    if (duplicate) fail(`${name}.png is byte-identical to ${duplicate}.png`);
    else seenHashes.set(digest, name);
  }
  // A black fringe usually comes from removing a black generation backdrop.
  // Only inspect the outer 2px ring, where legitimate dark interior detail
  // cannot be mistaken for a matte edge.
  // logo is the intentional brand exception: its four-colour app-mark may
  // include dark detail at the edge. Functional glyphs must never retain a
  // black matte after background removal.
  if (name !== "logo") for (let y = 0; y < height; y += 1) for (let x = 0; x < width; x += 1) {
    if (x > 1 && y > 1 && x < width - 2 && y < height - 2) continue;
    const i = (y * width + x) * 4;
    if (pixels[i + 3] > 16 && pixels[i] < 24 && pixels[i + 1] < 24 && pixels[i + 2] < 24) {
      fail(`${name}.png has a dark/black fringe on the canvas edge`);
      break;
    }
  }
  if (name !== "logo") {
    const meanBlue = blue / alphaTotal;
    const meanRed = red / alphaTotal;
    const meanGreen = green / alphaTotal;
    if (meanBlue < meanRed * 0.9 || meanBlue < meanGreen * 0.9) warn(`${name}.png is not blue-led (mean RGB ${meanRed.toFixed(0)},${meanGreen.toFixed(0)},${meanBlue.toFixed(0)})`);
  }
}

const seenHashes = new Map();

const actual = readdirSync(ICON_DIR).filter((file) => file.endsWith(".png")).map((file) => file.slice(0, -4)).sort();
for (const name of EXPECTED) if (!actual.includes(name)) fail(`missing ${name}.png`);
for (const name of actual) if (!EXPECTED.includes(name)) fail(`unexpected icon asset ${name}.png`);
for (const name of EXPECTED) if (actual.includes(name)) {
  try { inspect(name); } catch (error) { fail(`${name}.png: ${error.message}`); }
}

if (STRICT && warnings) errors += warnings;
if (errors) process.exitCode = 1;
else console.log(`Icon assets valid (${EXPECTED.length} files${warnings ? `, ${warnings} advisory warning(s)` : ""}).`);
