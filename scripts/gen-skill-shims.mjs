#!/usr/bin/env node
/**
 * gen-skill-shims.mjs — 从 .agents/skills/（canonical）同步完整内容到 .claude/skills/
 *
 * Claude Code 只读取 .claude/skills/，不识别跨工具标准目录 .agents/skills/。
 * 早期方案是写一行指针让模型自行跟随，但这要求模型在触发后主动调用 Read 工具，
 * 触发质量与执行可靠性都受损。因此本脚本直接将 canonical 的 SKILL.md 与
 * references/ 子目录原样镜像到 .claude/skills/<name>/，保证 Claude Code 在触发时
 * 拿到与其它工具完全一致的内容。canonical 仍是唯一编辑入口，本脚本负责幂等同步。
 *
 * 同步规则：
 *   - SKILL.md：整文件原样复制（含 frontmatter）
 *   - references/：递归复制整个子目录（canonical 无该目录则跳过）
 *   - 孤儿清理：canonical 已删除的技能，其 .claude/skills/<name>/ 整目录删除
 *   - 不触碰 CC 专属文件（如 evals/）；目前无此类文件，若未来新增需在 CLEAN_IGNORE 中声明
 *
 * 用法：node scripts/gen-skill-shims.mjs [--root <repo-root>]
 * 退出码：0 = 成功，1 = canonical 目录缺失
 */
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const ROOT = path.resolve(process.argv.includes('--root')
  ? process.argv[process.argv.indexOf('--root') + 1]
  : path.join(__dirname, '..'));

const CANONICAL_DIR = path.join(ROOT, '.agents', 'skills');
const SHIM_DIR = path.join(ROOT, '.claude', 'skills');

// 同步时需要保留但不在 canonical 中的 CC 专属文件/目录名（相对每个技能目录）。
// 目前为空。若未来重新引入 evals/ 等 CC 专属工件，在此声明以避免被孤儿清理误删。
const CLEAN_IGNORE = new Set([]);

function copyRecursive(src, dst) {
  if (!fs.existsSync(src)) return;
  fs.mkdirSync(dst, { recursive: true });
  for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
    const s = path.join(src, entry.name);
    const d = path.join(dst, entry.name);
    if (entry.isDirectory()) {
      copyRecursive(s, d);
    } else if (entry.isFile()) {
      fs.copyFileSync(s, d);
    }
  }
}

function removeRecursive(target) {
  if (!fs.existsSync(target)) return;
  fs.rmSync(target, { recursive: true, force: true });
}

// 同步单个技能目录：先清空 shim 目录内已知镜像文件，再从 canonical 复制。
// 不直接 rm -rf 整个 shim 目录，以保留可能存在的 CC 专属文件（CLEAN_IGNORE）。
function syncSkill(name) {
  const canonicalSkillDir = path.join(CANONICAL_DIR, name);
  const shimSkillDir = path.join(SHIM_DIR, name);

  // 清理 shim 目录中由本脚本生成的镜像文件（SKILL.md 与 references/）。
  // 其它文件（如未来可能存在的 evals/）保持不动。
  if (fs.existsSync(shimSkillDir)) {
    const shimSkillMd = path.join(shimSkillDir, 'SKILL.md');
    if (fs.existsSync(shimSkillMd)) fs.unlinkSync(shimSkillMd);
    const shimRefs = path.join(shimSkillDir, 'references');
    if (fs.existsSync(shimRefs)) removeRecursive(shimRefs);
  }

  fs.mkdirSync(shimSkillDir, { recursive: true });

  // 镜像 SKILL.md（整文件原样复制）
  const canonicalSkillMd = path.join(canonicalSkillDir, 'SKILL.md');
  if (!fs.existsSync(canonicalSkillMd)) {
    console.warn(`  ⚠ ${name}: 缺少 SKILL.md，跳过`);
    return false;
  }
  fs.copyFileSync(canonicalSkillMd, path.join(shimSkillDir, 'SKILL.md'));

  // 镜像 references/ 子目录（若存在）
  const canonicalRefs = path.join(canonicalSkillDir, 'references');
  if (fs.existsSync(canonicalRefs)) {
    copyRecursive(canonicalRefs, path.join(shimSkillDir, 'references'));
  }

  return true;
}

// 孤儿清理：canonical 已删除的技能，从 .claude/skills/ 移除其整个目录。
// 若 shim 目录中存在 CLEAN_IGNORE 中的文件，仅删除镜像文件、保留目录与专属文件。
function cleanOrphans(canonicalNames) {
  if (!fs.existsSync(SHIM_DIR)) return;
  for (const entry of fs.readdirSync(SHIM_DIR, { withFileTypes: true })) {
    if (!entry.isDirectory() || canonicalNames.includes(entry.name)) continue;
    const shimSkillDir = path.join(SHIM_DIR, entry.name);

    // 检查是否存在需要保留的 CC 专属文件
    const remaining = fs.readdirSync(shimSkillDir).filter(
      (f) => !CLEAN_IGNORE.has(f),
    );
    if (remaining.length === 0 && fs.readdirSync(shimSkillDir).length > 0) {
      // 仅有 CLEAN_IGNORE 文件 → 删除镜像产物，保留专属文件
      const shimSkillMd = path.join(shimSkillDir, 'SKILL.md');
      if (fs.existsSync(shimSkillMd)) fs.unlinkSync(shimSkillMd);
      const shimRefs = path.join(shimSkillDir, 'references');
      if (fs.existsSync(shimRefs)) removeRecursive(shimRefs);
      console.warn(`  ⚠ ${entry.name}: canonical 已删除，保留 CC 专属文件，移除镜像产物`);
    } else if (remaining.length === 0) {
      // 空目录或仅含镜像 → 整目录删除
      removeRecursive(shimSkillDir);
      console.warn(`  ⚠ ${entry.name}: canonical 已删除，移除孤儿目录`);
    } else {
      // 存在非镜像、非忽略的文件 → 仅删除镜像产物，保留未知文件
      const shimSkillMd = path.join(shimSkillDir, 'SKILL.md');
      if (fs.existsSync(shimSkillMd)) fs.unlinkSync(shimSkillMd);
      const shimRefs = path.join(shimSkillDir, 'references');
      if (fs.existsSync(shimRefs)) removeRecursive(shimRefs);
      console.warn(`  ⚠ ${entry.name}: canonical 已删除，但 shim 目录含未知文件，仅移除镜像产物`);
    }
  }
}

function main() {
  if (!fs.existsSync(CANONICAL_DIR)) {
    console.error(`gen-skill-shims: canonical 目录不存在 -> ${CANONICAL_DIR}`);
    process.exit(1);
  }

  const names = fs.readdirSync(CANONICAL_DIR, { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => e.name);

  let synced = 0;
  for (const name of names) {
    if (syncSkill(name)) synced += 1;
  }

  cleanOrphans(names);

  console.log(`gen-skill-shims: ${synced} 个技能已镜像 -> .claude/skills/`);
}

main();
