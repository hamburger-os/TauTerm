---
name: tauterm-docs
description: "Maintain TauTerm's documentation contract. Use for any change that can affect architecture, module responsibilities, user-visible workflows, build/platform/release behavior, security boundaries, product direction, README content, or documentation consistency. Read AGENTS.md first; this skill defines the documentation workflow rather than duplicating repository policy."
license: MIT
metadata:
  author: tauterm
  version: "2.0"
---

# TauTerm documentation workflow

Read the root `AGENTS.md` first. It owns the audience layers, source-of-truth map, and module ownership map. Do not copy those tables into another document.

## Goal

Documentation is part of the implementation contract. A code change and its affected design document should describe the same system at the same commit.

The maintainer-facing documents are intentionally concise and Chinese-first. They must make architectural changes reviewable without requiring the maintainer to inspect implementation code.

## Workflow

1. **Classify the change.** Decide whether it changes a public capability, current architecture, a module design, product direction, build/platform/release procedure, security reporting policy, or only implementation detail.
2. **Find the one owner.** Use the source-of-truth and module maps in `AGENTS.md`. If the fact already has an owner, edit that document instead of creating another copy.
3. **Update design with code.** When architecture or behavior changes, update the affected `docs/modules/*.md` document in the same task. Cross-module changes may update multiple documents, but each fact still has one owner.
4. **Link instead of repeat.** Supporting documents should link to the canonical owner. Do not maintain synchronized prose or duplicated tables.
5. **Remove stale material.** If a newer canonical document replaces an older one, delete the old document and repair links in the same change. Do not keep redirect/stub files unless an external compatibility requirement exists.
6. **Validate.** Run `npm run docs:check` and fix every error before completion.

## Maintainer document style

Files under `docs/README.md`, `docs/modules/`, and `docs/product/` are for the maintainer and should be Chinese-first.

A module design document should normally answer:

- 这个模块解决什么问题；
- 当前采用什么架构/方案；
- 关键数据流或生命周期是什么；
- 哪些边界不能被破坏；
- 与哪些模块存在明确接口；
- 哪些代码目录是实现锚点；
- 什么类型的改动必须同步更新本文。

Keep these documents at architecture and solution level. Prefer short diagrams, boundaries, states, and responsibilities over APIs or code excerpts. Do not mirror function names, struct fields, implementation steps, or large configuration samples unless they are themselves a stable contract.

## README rules

The root README is the public open-source landing page, not an architecture dump.

- Keep English and Chinese versions structurally aligned.
- Explain the project, supported workflow families, install entry, build entry, documentation navigation, contribution, security, and license.
- Link to canonical detailed documents instead of embedding platform/build/release internals.
- Do not place roadmap status, release history, or maintainer-only design details in the README.
- Do not use named competitor comparisons or unsupported performance claims.

## Community documents

Community procedures may be English, Chinese, or bilingual. They should remain actionable, but command definitions must defer to `package.json` rather than maintaining an exhaustive second command catalog.

Release notes stored as version-specific files are not allowed. `CHANGELOG.md` is the release-history source; the release workflow derives the GitHub Release body from it.

## AI documents and skills

`AGENTS.md` is the tool-neutral repository instruction entry. Skills contain specialized procedures or specifications and are loaded only when relevant.

Do not create parallel copies for individual agent products. If a tool needs an adapter, make it a minimal pointer to the canonical rule rather than a duplicated ruleset.

## Validation contract

Always run:

```bash
npm run docs:check
```

The checker enforces required documents, forbidden legacy duplicates, Markdown link integrity, README structural parity, maintainer-document language, changelog shape, and i18n key parity.

If a documentation rule cannot be mechanically checked, keep the rule in `AGENTS.md` or this skill rather than pretending the checker enforces it.
