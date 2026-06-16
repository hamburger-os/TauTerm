## Context

TauTerm v0.1 采用单会话架构（`AppState` 直接持有单个 `SerialSession`），前端使用散落的 `useState` hooks 管理状态。用户反馈串口连接后按回车键无响应，分析确认根因为 I/O 线程中的无缓冲 mpsc channel + 读优先调度导致写饥饿。界面采用初版 Liquid Glass 设计，但布局为固定侧边栏+终端的简单二段式，缺少专业终端模拟器应有的标签页、会话管理、搜索等功能。

目标是将 TauTerm 从串口助手升级为全功能终端模拟器，架构支持未来的 SSH/Telnet 扩展，UI 采用现代 Neon Dark Liquid Glass 设计。

## Goals / Non-Goals

**Goals:**
- 修复串口 I/O 写饥饿 bug（缓冲通道 + 公平调度）
- 实现 SessionManager 多会话架构，支持标签页创建/关闭/切换/重命名/拖拽排序
- 实现 Hub 式界面布局（会话侧边栏 + 标签栏 + 终端区 + 快速连接栏 + 传输面板 + 状态栏）
- 实现 Liquid Glass v2 设计系统（Neon Dark 主题 + Framer Motion 动画）
- 实现终端搜索（Ctrl+F）、命令面板（Ctrl+Shift+P）、快捷键系统
- 实现会话持久化（JSON 启动恢复）
- 文件传输增强（Dropzone 拖拽 + 扫光动画）
- 架构预留 SSH/Telnet 扩展点

**Non-Goals:**
- SSH/Telnet 协议实现（架构预留，不实现功能）
- SCP/SFTP 文件传输
- ZModem/Kermit 协议
- 终端会话录制
- 宏录制/脚本
- 分屏/窗格（pane splitting）
- 插件系统

## Decisions

### D1: SessionManager 架构

**选择**: 在 Rust 端引入 `SessionManager` 作为全局单例管理器。

```rust
struct SessionManager {
    sessions: HashMap<String, SessionHandle>,  // TabId → Session
    active_id: Option<String>,
    tab_order: Vec<String>,
}

struct SessionHandle {
    id: String,
    name: String,
    connection: Box<dyn TermSession>,
    write_tx: SyncSender<IoCmd>,        // 改为缓冲通道
    io_cancel_tx: Option<oneshot::Sender<()>>,
    io_thread: Option<JoinHandle<()>>,
    state: SessionState,
}
```

**备选方案**:
- A) 每个标签页独立 process — 隔离性最好但资源开销大，不适合
- B) 单一 I/O 线程多路复用 — 实现复杂，断连一个影响全部
- ✅ 每标签页独立 I/O 线程 + SessionManager 协调 — 最佳平衡

### D2: 通道类型切换

**选择**: 从 `mpsc::channel()`（无缓冲 rendezvous）改为 `mpsc::sync_channel(32)`（缓冲 32 条消息）。

**理由**:
- 无缓冲通道要求发送方和接收方同步，写操作会阻塞 Tauri 命令线程
- 32 条缓冲足够覆盖用户快速输入的瞬态突发，不会因 I/O 线程 10ms tick 延迟阻塞
- 32 是经验值：不会吃太多内存，也不会因为缓冲满而丢写

### D3: I/O 循环公平调度

**选择**: 修改 I/O 循环为读-写交替，不再读优先。

```rust
loop {
    // 1. 检查取消
    // 2. 尝试读取（非阻塞）
    // 3. 尝试写入（非阻塞，处理所有排队的写操作）
    // 4. 短暂 sleep (1ms 替代 10ms)
}
```

### D4: 前端状态管理

**选择**: React Context + useReducer 替代散落的 useState。

- `SessionContext`: 所有会话状态、活跃标签页
- `ThemeContext`: 当前主题、主题列表
- `TransferContext`: 文件传输状态

**备选方案**:
- A) Zustand — 轻量但引入新依赖
- B) Redux — 过重，不适合此规模
- ✅ Context + useReducer — 零依赖，够用

### D5: Framer Motion 集成

**选择**: 引入 `framer-motion` 作为动画引擎。

**理由**: 用户要求的 Liquid Glass 交互（hover 光晕追踪、呼吸灯涟漪、扫光、拖拽阻尼、Dropzone 弹性动画）用纯 CSS animation 实现成本极高且体验差。Framer Motion 的 `whileHover`、`AnimatePresence`、`drag`、手势系统可以优雅实现。

**权衡**: ~35KB gzip 额外体积，但对于桌面应用（非 web）完全可接受。

### D6: 会话持久化格式

**选择**: JSON 文件存储到 `$APPDATA/TauTerm/sessions.json`（Windows）/ `~/.config/TauTerm/sessions.json`（Linux/macOS）。

**理由**: 简单可靠，无需 SQLite 依赖。格式可读可手动编辑，版本迁移容易。

### D7: 主题系统实现

**选择**: `data-theme` 属性 + CSS 变量动态注入。

```html
<html data-theme="neon-dark">
```

```css
:root[data-theme="neon-dark"] {
  --accent-primary: #00d4aa;
  --glass-bg: rgba(0, 212, 170, 0.04);
  /* ... */
}
```

主题切换时 JavaScript 只需修改 `document.documentElement.dataset.theme`，CSS 变量自动级联。

## Risks / Trade-offs

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| Framer Motion 性能 | 终端大量输出时动画可能掉帧 | 动画仅用于 UI chrome，终端区域本身不使用 Framer Motion |
| sync_channel(32) 缓冲区满 | 极端快速输入时可能丢写 | 32 条 × 平均 16 字节 = 512B，远小于终端输入速率；必要时可增至 64 |
| SessionManager 锁竞争 | 多标签页并发写入同一 Mutex | SessionManager 使用 `RwLock`，写操作仅锁单个 SessionHandle |
| JSON 持久化损坏 | 会话文件损坏导致启动失败 | 启动时 try-catch，损坏文件回退到空列表，备份旧文件为 `.bak` |
| xterm.js 多实例内存 | 5+ 标签页可能占用 500MB+ | 限制最多 10 个标签页，非活跃标签页可降低 scrollback buffer |

## Migration Plan

1. 创建 `SessionManager` + 重构 Rust 后端（保留旧 commands.rs 接口但重路由到 SessionManager）
2. 重构前端为 Context + useReducer，迁移 App.tsx → AppShell 组件树
3. 逐步替换 UI 组件，引入 Framer Motion
4. 实现标签页、搜索、命令面板等新功能
5. 添加主题系统和会话持久化
6. 测试和 bug 修复

**回滚**: 所有变更在同一分支，未合并前可随时回退。无数据库迁移风险。

## Open Questions

- 主题预设数量：先做 Neon Dark + 2 套备选（Ocean Blue、Sunset Amber），后续版本扩展？
- 会话保存时是否包含终端 scrollback buffer？（推荐：不保存，仅保存连接参数）
- 标签页关闭时的确认对话框？对于"已连接"的标签页，关闭前是否需要确认？
