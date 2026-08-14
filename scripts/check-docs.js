#!/usr/bin/env node
/**
 * check-docs.js — TauTerm 文档一致性校验
 *
 * 校验项：
 *  1. README.md / README.zh-CN.md 的 ## 标题序列一一对应（双语文档镜像）
 *  2. 两版 README 无竞品名 / 负面对比表述（禁拉踩）
 *  3. 所有文档的相对 markdown 链接指向存在的文件
 *  4. CHANGELOG.md 为 Keep a Changelog 格式且含版本段
 *  5. README 篇幅警告（> 400 行）
 *
 * 用法：node scripts/check-docs.js [--root <repo-root>]
 * 退出码：0 = 通过，1 = 存在错误
 */
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const ROOT = path.resolve(process.argv.includes('--root')
  ? process.argv[process.argv.indexOf('--root') + 1]
  : path.join(__dirname, '..'));

const DOC_FILES = [
  'README.md',
  'README.zh-CN.md',
  'docs/ARCHITECTURE.md',
  'docs/BUILDING.md',
  'CONTRIBUTING.md',
  'CHANGELOG.md',
];

const BANNED = /MobaXterm|WindTerm|VOFA\+?|Tabby|Electron/i;

const errors = [];
const warnings = [];

function fail(msg) { errors.push(msg); }
function warn(msg) { warnings.push(msg); }

function read(rel) {
  try {
    return fs.readFileSync(path.join(ROOT, rel), 'utf8');
  } catch {
    fail(`文件不存在: ${rel}`);
    return null;
  }
}

function headings(md) {
  return (md.match(/^##\s+(.+)$/gm) || []).map((l) => l.replace(/^##\s+/, '').trim());
}

/** GitHub 风格的标题 slug 近似实现（用于锚点校验） */
function slugify(h) {
  return h
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s-]/gu, '')
    .trim()
    .replace(/\s+/g, '-');
}

function checkLinks(md, rel) {
  const base = path.dirname(path.join(ROOT, rel));
  const linkRe = /\[[^\]]*\]\(([^)]+)\)/g;
  let m;
  while ((m = linkRe.exec(md)) !== null) {
    const target = m[1].trim();
    if (/^(https?:|mailto:)/.test(target)) continue;
    const [filePart, anchor] = target.split('#');
    const resolved = filePart
      ? path.resolve(base, filePart)
      : path.join(ROOT, rel);
    if (filePart) {
      if (!fs.existsSync(resolved)) {
        fail(`${rel}: 链接目标不存在 -> ${target}`);
        continue;
      }
    }
    if (anchor) {
      const targetFile = filePart
        ? path.relative(ROOT, resolved).replace(/\\/g, '/')
        : rel;
      const content = read(targetFile);
      if (content) {
        const slugs = new Set(
          content.match(/^#{1,6}\s+(.+)$/gm).map((l) => slugify(l.replace(/^#{1,6}\s+/, ''))),
        );
        if (!slugs.has(slugify(anchor))) {
          warn(`${rel}: 锚点可能失效 -> ${target}（${targetFile} 中未找到匹配标题）`);
        }
      }
    }
  }
}

function main() {
  // 1. 镜像对齐（结构对齐：标题数量一致；标题文本按语言翻译，不要求逐字相同）
  const en = read('README.md');
  const zh = read('README.zh-CN.md');
  if (en && zh) {
    const enH = headings(en);
    const zhH = headings(zh);
    if (enH.length !== zhH.length) {
      fail(`README.md 与 README.zh-CN.md 的 ## 标题数量不一致（英文版 ${enH.length} 个 vs 中文版 ${zhH.length} 个）` +
        `\n  英文版: ${enH.join(' | ')}` +
        `\n  中文版: ${zhH.join(' | ')}`);
    }
    for (const h of zhH) {
      if (!/\p{Script=Han}/u.test(h)) {
        warn(`README.zh-CN.md: 标题「${h}」不含中文，疑似未翻译的英文标题粘贴`);
      }
    }
  }

  // 2. 禁拉踩
  for (const rel of ['README.md', 'README.zh-CN.md']) {
    const md = read(rel);
    if (!md) continue;
    const hits = [...md.matchAll(new RegExp(BANNED, 'g'))];
    if (hits.length) {
      fail(`${rel}: 命中禁拉踩词 ${hits.length} 处（${[...new Set(hits.map((h) => h[0]))].join(', ')}）— 只描述自身优势，不提竞品`);
    }
  }

  // 3. 链接有效
  for (const rel of DOC_FILES) {
    const md = read(rel);
    if (md) checkLinks(md, rel);
  }

  // 4. CHANGELOG 格式
  const cl = read('CHANGELOG.md');
  if (cl) {
    if (!/Keep a Changelog/.test(cl)) fail('CHANGELOG.md: 缺少 Keep a Changelog 格式声明');
    if (!/^##\s*\[[\d.]+\]/m.test(cl)) fail('CHANGELOG.md: 未找到版本段（## [x.y.z]）');
  }

  // 5. README 篇幅
  for (const rel of ['README.md', 'README.zh-CN.md']) {
    const md = read(rel);
    if (!md) continue;
    const lines = md.split('\n').length;
    if (lines > 400) warn(`${rel}: ${lines} 行，超出营销文档精简目标（≤ ~350 行）`);
  }

  // 输出
  const passCount = 5 - errors.length;
  console.log(`check-docs: ${passCount}/5 项通过`);
  for (const e of errors) console.log(`  ✗ ${e}`);
  for (const w of warnings) console.log(`  ⚠ ${w}`);
  if (errors.length === 0) {
    console.log('  ✓ 文档一致性校验通过');
    process.exit(0);
  }
  console.log(`  ${errors.length} 个错误，文档未通过校验`);
  process.exit(1);
}

main();
