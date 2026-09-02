#!/usr/bin/env node

/** Generate a local 12/14/18/24px dark/light review board for the icon assets.
 * Use check-icons.mjs for the machine-enforced alpha-core and optical-frame
 * dimensions; this board is for visual weight, semantic silhouette, and theme
 * contrast review at the actual UI sizes.
 */
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const names = readdirSync("src/assets/icons")
  .filter((file) => file.endsWith(".png") && file !== "logo.png")
  .map((file) => file.slice(0, -4))
  .sort();
const sizes = [12, 14, 18, 24];
const contract = JSON.parse(readFileSync("src/assets/icons/style-contract.json", "utf8"));
const familyOf = (name) => contract.microControlKeys.includes(name) ? "micro" : "functional";
const iconImages = (name) => sizes.map((size) => `<img src="../src/assets/icons/${name}.png" width="${size}" height="${size}" alt="${name} ${size}px">`).join("");
const cells = names.map((name) => `<article><code>${name}</code><small>${familyOf(name)}</small>${iconImages(name)}</article>`).join("\n");
const anchorGroup = (label, anchors) => `<div><strong>${label}</strong>${anchors.map((name) => `<span><code>${name}</code>${iconImages(name)}</span>`).join("")}</div>`;
const anchors = `<aside class="anchors"><b>Mandatory style anchors</b>${anchorGroup("functional", contract.functionalReferences)}${anchorGroup("micro", contract.microControlReferences)}</aside>`;
const logoCells = [16, 32].map((size) => `<img src="../src/assets/icons/logo.png" width="${size}" height="${size}" alt="logo ${size}px">`).join("");
const html = `<!doctype html><meta charset="utf-8"><title>TauTerm icon review</title><style>
body{font:13px system-ui;margin:24px;background:#10131a;color:#e9eef8}h1{font-size:20px}.sizes{color:#9aa8bd}main{display:grid;grid-template-columns:repeat(auto-fill,minmax(210px,1fr));gap:10px}article{display:flex;align-items:center;gap:12px;padding:10px;border:1px solid #2b3547;border-radius:10px;background:#17202d}article small{width:54px;color:#8291a8}code{width:76px;overflow-wrap:anywhere;color:#b9d4ff}img{object-fit:contain}.anchors{position:sticky;top:0;z-index:2;padding:12px;margin:12px 0;background:#10131af2;border:1px solid #46546a;border-radius:12px}.anchors>div,.anchors span{display:flex;align-items:center;gap:12px}.anchors>div{margin-top:8px}.anchors strong{width:76px}.anchors span code{width:110px}.light{margin-top:24px;padding:16px;background:#edf3fb;color:#182237}.light article{background:#fff;border-color:#ccd8e8}.light code{color:#28588e}.light .anchors{background:#edf3fbf2;border-color:#a9b9cf}</style><h1>TauTerm icon review</h1><p class="sizes">Functional icons: 12 · 14 · 18 · 24 px. Compare every candidate with its fixed family anchors for visual weight, glass thickness, semantic silhouette, direction and theme contrast. Run <code>npm run check:icons -- --strict</code> first.</p>${anchors}<h2>Brand logo: 16 · 32 px</h2><article><code>logo</code>${logoCells}</article><main>${cells}</main><section class="light"><h1>Light surface</h1>${anchors}<h2>Brand logo: 16 · 32 px</h2><article><code>logo</code>${logoCells}</article><main>${cells}</main></section>`;
const output = resolve(process.argv[2] ?? "dist/icon-preview.html");
mkdirSync(resolve(output, ".."), { recursive: true });
writeFileSync(output, html);
console.log(`Wrote ${output}`);
