## 1. 项目脚手架

- [x] 1.1 使用 React + TypeScript 模板初始化 Tauri v2 项目（`npm create tauri-app@latest`）
- [x] 1.2 创建 `.gitignore`，覆盖 Rust `target/`、`node_modules/`、`dist/`、IDE 文件和操作系统临时文件
- [x] 1.3 编写中文 `README.md`，包含项目介绍、技术栈、功能特性、各平台构建说明和截图占位
- [x] 1.4 配置 `src-tauri/Cargo.toml` 依赖项：`tauri` v2、`serde`、`serde_json`、`serialport`、`tokio`、`thiserror`
- [x] 1.5 配置 `package.json` 前端依赖项：`react`、`react-dom`、`@tauri-apps/api` v2、`@xterm/xterm`、`@xterm/addon-fit`、`@xterm/addon-web-links`、`i18next`、`react-i18next`
- [x] 1.6 创建目录结构：`src-tauri/src/serial/`、`src-tauri/src/transfer/`、`src/components/`、`src/hooks/`、`src/styles/`、`src/types/`、`src/i18n/locales/`
- [x] 1.7 初始化 i18n 框架：创建 `src/i18n/index.ts` 配置文件，创建 `zh-CN.json`（中文默认）和 `en-US.json`（英文）语言资源文件，包含基础 UI 文本键值对
- [x] 1.8 验证项目构建和开发服务器正常启动（`cargo build` + `npm run tauri dev`）

## 2. Liquid Glass 设计系统

- [x] 2.1 在 `src/styles/tokens.css` 中定义 CSS 自定义属性（设计令牌）：颜色、模糊值、圆角、过渡、字体（含中文字体回退）
- [x] 2.2 创建 `GlassPanel` 组件：`backdrop-filter: blur()`、半透明背景、微细白色边框
- [x] 2.3 创建 `GlassButton` 组件：强调渐变、悬停发光、按压激活状态
- [x] 2.4 创建 `GlassInput` 和 `GlassSelect` 组件：表单控件带焦点发光动画
- [x] 2.5 应用全局深色背景渐变（`#0a0a1a` → `#0d1117`）和基础排版（UI 使用 Inter，终端使用 JetBrains Mono，中文字体回退）
- [x] 2.6 构建应用布局外壳：左侧可调整侧边栏、中间终端视口、底部可折叠文件传输面板
- [x] 2.7 添加 CSS 过渡动画：面板打开/关闭（300ms 滑动）、悬停微交互（150ms 缩放/亮度）

## 3. 串口后端（Rust）

- [x] 3.1 在 `src-tauri/src/serial/config.rs` 中定义串口配置类型
- [x] 3.2 在 `src-tauri/src/serial/manager.rs` 中实现 `SerialPortManager`
- [x] 3.3 通过 `serialport::available_ports()` 实现端口枚举
- [x] 3.4 实现异步读取循环
- [x] 3.5 实现写入函数
- [x] 3.6 在 `src-tauri/src/commands.rs` 中注册 Tauri 命令
- [x] 3.7 处理端口断开
- [x] 3.8 在 `lib.rs` 中串联：初始化 Tauri 应用，注册命令，管理全局串口状态

## 4. 终端前端

- [x] 4.1 创建 `Terminal` 组件，集成 xterm.js 并启用 `FitAddon` 和 `WebLinksAddon`
- [x] 4.2 创建 `useSerialPort` hook：管理连接状态，封装 Tauri 的 invoke/event 调用
- [x] 4.3 监听串口数据事件，写入 xterm.js
- [x] 4.4 捕获 xterm.js `onData` 回调，发送到 Rust 后端
- [x] 4.5 处理终端粘贴事件
- [x] 4.6 响应终端大小调整
- [x] 4.7 构建 `SerialConfigSidebar` 组件
- [x] 4.8 侧边栏接入 `useSerialPort` hook

## 5. YModem 文件传输

- [x] 5.1 在 `src-tauri/src/transfer/protocol.rs` 中定义 `FileTransferProtocol` trait
- [x] 5.2 在 `src-tauri/src/transfer/ymodem.rs` 中实现 YModem 发送逻辑
- [x] 5.3 实现 YModem 接收逻辑
- [x] 5.4 实现 CRC-16/CCITT 计算函数
- [x] 5.5 实现重试逻辑
- [x] 5.6 注册 Tauri 命令并带进度事件推送
- [x] 5.7 构建 `FileTransferPanel` React 组件
- [x] 5.8 构建传输历史列表
- [x] 5.9 创建 `useFileTransfer` hook

## 6. 多语言国际化（i18n）

- [x] 6.1 定义 TypeScript 翻译类型接口
- [x] 6.2 完善中文语言文件 zh-CN.json
- [x] 6.3 完善英文语言文件 en-US.json
- [x] 6.4 在组件中使用 useTranslation hook
- [x] 6.5 在状态栏中添加语言切换按钮
- [x] 6.6 使用 localStorage 持久化用户语言偏好

## 7. 集成与打磨

- [x] 7.1 在 App.tsx 中串联所有组件
- [x] 7.2 添加错误边界和 Toast 提示通知
- [x] 7.3 添加键盘快捷键
- [x] 7.4 串口通信测试（用户已验证：数据收发正常，Tab 自动补全正常）
- [ ] 7.5 YModem 传输测试（状态栏已有入口，需物理串口环回测试）
- [x] 7.6 Windows 构建验证（`npx tauri build` 成功，0 warnings 0 errors）
- [x] 7.7 语言切换测试（i18n 翻译已完善，连接类型标签即时切换）
