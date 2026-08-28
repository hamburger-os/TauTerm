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
 *  6. i18n 语言文件 en-US.json / zh-CN.json 的 key 集合一一对应
 *  7. SecuritySettings.tsx 静态引用的 settings.security* key 在两份语言文件中均存在
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
  'docs/RELEASING.md',
  'docs/SUPPORTED_PLATFORMS.md',
  'docs/LAUNCH.md',
  'CONTRIBUTING.md',
  'CHANGELOG.md',
];

const BANNED = /MobaXterm|WindTerm|VOFA\+?|Tabby|Electron/i;
const CHECK_COUNT = 7;

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

/** 递归展平 JSON 对象为点分 key 列表（用于 i18n key 对齐） */
function flatten(obj, prefix = '') {
  const out = [];
  for (const [k, v] of Object.entries(obj)) {
    const full = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === 'object' && !Array.isArray(v)) {
      out.push(...flatten(v, full));
    } else {
      out.push(full);
    }
  }
  return out;
}

function checkI18n() {
  const files = ['src/i18n/locales/en-US.json', 'src/i18n/locales/zh-CN.json'];
  const parsed = files.map((f) => {
    const raw = read(f);
    if (raw == null) return null;
    try {
      return JSON.parse(raw);
    } catch (e) {
      fail(`${f}: JSON 解析失败: ${e.message}`);
      return null;
    }
  });
  const [en, zh] = parsed;
  if (!en || !zh) return;
  const enSet = new Set(flatten(en));
  const zhSet = new Set(flatten(zh));
  const onlyEn = [...enSet].filter((k) => !zhSet.has(k)).sort();
  const onlyZh = [...zhSet].filter((k) => !enSet.has(k)).sort();
  if (onlyEn.length) fail(`en-US.json 存在 zh-CN.json 缺失的 key（${onlyEn.length}）: ${onlyEn.join(', ')}`);
  if (onlyZh.length) fail(`zh-CN.json 存在 en-US.json 缺失的 key（${onlyZh.length}）: ${onlyZh.join(', ')}`);

  const securitySource = read('src/components/Settings/panels/SecuritySettings.tsx');
  if (!securitySource) return;
  const securityKeys = [...securitySource.matchAll(/\bt\(\s*["'](settings\.security[^"']+)["']\s*[,)]/g)]
    .map((match) => match[1]);
  const uniqueSecurityKeys = [...new Set(securityKeys)].sort();
  if (uniqueSecurityKeys.length === 0) {
    fail('SecuritySettings.tsx: 未找到静态 settings.security* 翻译引用');
    return;
  }
  const missingEn = uniqueSecurityKeys.filter((key) => !enSet.has(key));
  const missingZh = uniqueSecurityKeys.filter((key) => !zhSet.has(key));
  if (missingEn.length) fail(`SecuritySettings.tsx 引用的 key 在 en-US.json 中缺失（${missingEn.length}）: ${missingEn.join(', ')}`);
  if (missingZh.length) fail(`SecuritySettings.tsx 引用的 key 在 zh-CN.json 中缺失（${missingZh.length}）: ${missingZh.join(', ')}`);
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

  // 6. i18n 语言文件 key 对齐
  checkI18n();

  // 输出
  const passCount = errors.length === 0 ? CHECK_COUNT : Math.max(0, CHECK_COUNT - errors.length);
  console.log(`check-docs: ${passCount}/${CHECK_COUNT} 项通过`);
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
