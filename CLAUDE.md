# CLAUDE.md

TauTerm 开发指南 — AI 辅助开发参考。

## 项目概述

TauTerm 是基于 **Tauri v2** (Rust + React 18 + TypeScript) 的跨平台微内核终端模拟器。内核不含协议实现，所有会话类型（串口、SSH、Telnet 等）均作为独立插件注册。

## 构建与运行

```bash
npm install                # 安装依赖
npm run tauri dev          # 开发模式
npm run tauri build        # 生产构建
npm run build              # 仅前端构建
cd src-tauri && cargo test # Rust 测试
cd src-tauri && cargo clippy -- -D warnings
```

## 项目结构要点

- `src-tauri/src/channel/` — I/O 通道抽象（Channel trait, io_loop, serial_comm）
- `src-tauri/src/kernel/comm_handle.rs` — CommHandle trait（协议无关通信抽象）
- `src-tauri/src/kernel/script_engine/` — Lua 5.4 脚本运行时（mod/lua_api/sandbox/codegen）
  - Lua API 函数：`send(data)`, `sleep(ms)`, `log(message)`, `on_data(pattern, cb)`, `register_timer(id, ms, cb)`, `unregister_timer(id)`, `regex_find(pattern, str)`, `_time_ms()`, `_datetime_iso()`, `_datetime_format(fmt)`
  - 沙箱已移除：`os`, `io`, `require`, `dofile`, `loadfile`, `debug`
- `src-tauri/src/kernel/session_store.rs` — 会话生命周期管理
- `src-tauri/src/commands.rs` — Tauri 命令（前后端接口）
- `src/components/SendBar/` — 发送栏 4 模式（basic/command/auto-reply/script）
  - `BasicSend.tsx` — 基础发送（Text/HEX、换行符、循环发送、历史记录）
  - `CommandPanel.tsx` — 指令面板（预定义命令序列、拖拽排序、循环执行）
  - `AutoReplyPanel.tsx` — 自动应答规则配置面板（5 种匹配模式、10 种宏、HEX 二进制匹配）
  - `AutoReplyRuleEditor.tsx` — 规则编辑器（多条件组合、取反、序列回复、定时器触发）
  - `ScriptEditor.tsx` — Lua 脚本编辑器（手写代码、日志面板、"转换为脚本"工作流）
  - `SendBarContext.tsx` — 发送栏全局状态（useReducer + localStorage 持久化）
  - `types.ts` — 发送栏类型定义（SendBarMode, AutoReplyRule, MatchCondition 等）
  - `builtinRules.ts` — 10 套内置自动应答配置（AT 命令、Modbus、传感器遥测等）
  - `builtinScripts.ts` — 10 个内置 Lua 脚本示例（回显、定时器、NMEA 解析等）
  - `MatchTester.tsx` — 匹配表达式实时测试器（5 种模式 × text/hex，通过 Tauri invoke 调用后端）
  - `MacroPicker.tsx` — 回复动作宏插入器（14 种模板宏）
- `src/hooks/usePointerDragReorder.ts` — 通用指针事件拖拽排序 hook（AutoReplyPanel / ReplyActionEditor 共用）
- `src-tauri/src/transfer/` — 文件传输子系统（X/Y/ZModem 协议）
- `src-tauri/src/security/` — 凭据存储（keyring + AES-256-GCM）
- `src-tauri/src/virtual_port/` — 虚拟串口（com0com 驱动集成、端口对生命周期、双向桥接）
- `src-tauri/src/plugins/serial/` — 串口插件（ProtocolAdapter trait 实现）
- `src/core/` — 前端内核 API（plugin-registry, tab-host, config-store, event-bus）
- `src/renderers/` — 内容适配器（TerminalRenderer 等）
- `src/context/` — React Context（Session, Toast, Transfer）
- `src/styles/tokens.css` — CSS 设计 token（主题变量）

## 代码规范

### 设计系统 — Liquid Glass v3

**最重要的规则：永远不要硬编码颜色值。** 始终使用 CSS 自定义属性。

```css
/* ✅ 正确 */  color: var(--text-primary); background: var(--glass-fill);
/* ❌ 错误 */  color: #e0e0e0; background: rgba(255,255,255,0.05);
```

完整 token 参考见 `docs/theme-guide.md`。新 CSS 必须兼容全部 3 个主题（google-glow, obsidian, frosted）。

### CSS Modules

所有组件样式使用 CSS Modules（`*.module.css`）。全局工具类定义在 `src/styles/global.css`。

### i18n

所有用户可见文本必须通过 `t()` 函数翻译：
```tsx
const { t } = useTranslation();
<span>{t("sendBar.save")}</span>
```

### Lua 沙箱规则

脚本 VM 中已移除 `os`, `io`, `require`, `dofile`, `loadfile`, `debug`。`load` 保留供 EXPR 宏的算术安全求值使用（表达式经字符白名单校验，风险可控）。向 Lua API 添加新函数时，确保不会引入 I/O 能力或绕过沙箱限制。

### 协作式关闭模式

所有长时间运行的后台线程必须响应关闭信号：
- 使用 `AtomicBool` 作为关闭标志
- 主循环使用 `recv_timeout` 而非 `recv`
- `sleep()` 类调用分片为 ≤50ms 的块

## 关键设计决策

1. **"始终挂载"面板**：SendBar 4 个子面板始终在 DOM 中，通过 CSS `display` 切换可见性，保留各面板状态。
2. **指针事件拖拽**：使用 Raw Pointer Events 而非 HTML5 Drag API，兼容 Tauri WebView2。
3. **CommHandle 协议无关抽象**：脚本引擎通过 `CommHandle` trait 与底层通信，不感知协议差异。
4. **代码生成管道**：`AutoReplyRule[]` → Tauri invoke → `codegen::rules_to_lua_script()` → Lua 字符串 → 注入 VM。
5. **数据扇出**：`CommHandle::notify_receive()` 通过回调列表将数据同时传递给终端和脚本引擎。
