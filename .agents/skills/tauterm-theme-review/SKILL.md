---
name: tauterm-theme-review
description: "Audit TauTerm UI for theme, Liquid Glass material, and rendering-performance compliance. This skill is QA-only and MUST load tauterm-theme/SKILL.md as the single source of truth before judging any rule."
license: MIT
metadata:
  author: tauterm
  version: "3.6"
---

# TauTerm 主题与渲染审查

> **唯一规范源**：开始审查前必须读取 `.agents/skills/tauterm-theme/SKILL.md`。  
> 本文件只定义审查流程、扫描方法和报告格式，**不得复制材质/性能规则或维护第二份规则表**。

## 审查范围

默认检查 `src/components/`、`src/renderers/`、`src/styles/`、`src/context/ThemeContext.tsx`、`src/App.tsx`。

## 流程

### 1. 读取 SSOT

先读 `tauterm-theme/SKILL.md`，以其中当前版本的 surface、backdrop、performance/motion、token、animation/transition、Terminal/SplitView、cross-theme 规则作为唯一判断依据。

### 2. 快速扫描

```bash
rg 'backdrop-filter' src/components src/renderers --glob '*.module.css'
rg 'transition:\s*all' src --glob '*.css'
rg 'filter:\s*blur|mix-blend-mode|will-change' src --glob '*.css' --glob '*.tsx'
rg 'glass-blur|bg-orb-blur' src
rg 'liquid-glass-panel|liquid-glass-content|liquid-glass-accent|liquid-glass-float|liquid-glass' src --glob '*.tsx'
rg 'data-performance|data-motion|tauterm-performance-mode' src
rg 'theme-(chrome|panel|content|card|float|control).*veil|theme-(chrome|float)-veil-compat|liquid-clear-|liquid-specular-' src/styles
rg 'pane(Frame|Header|Content)Radius|dockedBorderRadii|border-radius' src/components/Layout/SplitView* src/components/Terminal/TerminalView.tsx
rg '#[0-9a-fA-F]{3,8}|rgba?\(' src/components src/renderers --glob '*.css' --glob '*.tsx'
rg '#FE3734|#F4BA00|#02BE66|#0B8AFF|#4285F4|#EA4335|#FBBC05|#34A853' src --glob '*.css' --glob '*.tsx' --glob '*.ts'
```

硬编码命中必须再按 SSOT 的允许例外人工筛选。

### 3. 重点路径

优先人工审查：

1. TerminalView / Terminal.module.css
2. SplitView
3. GoogleGlowBackground / ThemeContext motion
4. Structural Panel ownership（左右 Sidebar / SendBar / TargetBar）与重复 glass
5. SendBar 左侧竖排布局冻结 / TargetBar / disabled controls
6. Toolbar / Sidebar / RightSidebar / StatusBar
7. Dialog / Popover / ContextMenu
8. 可扩展的大面积面板
9. Frosted theme
10. Compatibility mode（重点检查无 blur 时是否反而最透明）
11. SplitView 外角/内角几何一致性

### 4. 验证矩阵

必须覆盖：

- 3 themes × 3 performance modes
- single pane / 2 panes / 4 panes
- 多 xterm 后台挂载
- focused / unfocused / hidden
- reduced motion
- 高速终端输出
- resize / split drag

### 5. 报告

用中文输出：

```markdown
# TauTerm 主题/性能审查

**范围**：
**SSOT 版本**：tauterm-theme vX
**验证矩阵**：

## 摘要
| 严重程度 | 数量 |
|---|---:|
| CRITICAL | |
| HIGH | |
| MEDIUM | |
| LOW | |

## 发现
| 严重度 | 文件:行 | 违反的 SSOT 小节 | 问题 | 修复 |
|---|---|---|---|---|

## 验证
- npm run build:
- 主题:
- 性能档:
- 分屏:
- 后台动画:
```

严重度按影响判定：

- **CRITICAL**：主题不可用、内容不可读、持续高 GPU/严重卡顿、Compatibility 失效
- **HIGH**：违反 surface/backdrop 红线、重新引入大面积 blur/持续合成
- **MEDIUM**：不必要动画、transition/all、层级/active 状态不一致
- **LOW**：轻微 token、视觉一致性、可维护性问题

不要在本 skill 中新增新的视觉规则；发现规范缺口时修改 `tauterm-theme/SKILL.md`。
