# 核心与会话架构

## 目标

核心层负责把不同协议统一成可管理的 Session，并提供插件注册、运行时 I/O 能力、状态、配置和公共生命周期。它不解释任何具体协议的业务语义。

TauTerm 的插件模型是 **显式编译期内建插件架构**，不是运行时第三方插件市场。插件随应用一起编译、测试和发布；插件边界用于隔离协议语义、UI contribution 与生命周期，而不是引入动态 ABI、热卸载、第三方代码沙箱或兼容层。

## 当前方案

后端以 Rust `SessionStore` 作为用户可见 Session 生命周期的权威所有者。`PluginRuntime` 是插件身份、canonical manifest、通用 `ProtocolAdapter` 与类型化 contribution 的唯一后端运行时目录；`plugins/catalog.rs` 是唯一了解完整内建插件集合的后端 composition 模块，负责把 manifest、Adapter、连接/配置/断开 contribution、插件专属 IPC wiring 以及需要宿主资源的插件初始化组装成 `PluginRuntime`。`AppState` 与应用 bootstrap 不再为 Serial、SSH、Telnet 等维护平行 adapter 字段、逐插件初始化分支或插件命令清单。协议 Adapter 负责建立协议资源，并以 `ProtocolConnection` 返回 `DataPlaneRuntime`、可选 `SessionService`、显式 `FileTransfer` capability、可选子终端工厂和 attach hook 等明确能力；核心把普通 DataPlane 绑定为 `SessionDataPlane + SessionIo`，统一承担收发、订阅、统计、终端 resize 和独占 I/O lease。Container Session 可以额外提供协议无关 `AutomationIo` capability，使多路协议参与 SendBar/Lua 而不伪造一个不存在的主 DataPlane。

Transport、Protocol、Session Runtime 三层职责固定：Transport 只拥有串口/TCP/UDP/PTY 等物理或系统资源；Protocol 解释 Telnet、Modbus、SSH 等协议语义；Session Runtime 负责生命周期、事件、日志、脚本、统计、子连接和取消。协议不得复制公共 Session 生命周期，Session Runtime 也不得解析协议字段。

一个配置可以对应单个 Session，也可以由协议提供“一个父配置、多子终端/peer”的工厂能力。公共核心负责子连接编号、活动根 Session 资源预算、状态与清理，协议只负责创建自身资源。Network peer 和 SSH/local-shell 子终端都使用同一 `SubConnection` 生命周期；是否显示为独立 tab 属于表现能力，而不是第二套后端模型。

Saved Session Library 是独立的版本化磁盘配置集合，不受活动运行时数量预算约束。Runtime Session 只消费稳定配置，不把 socket、PTY、DataPlane、任务、计数器等运行态写回 Library；Library 只允许通过显式保存、删除、重命名与参数事务入口修改。Session Library、ConfigStore 与 SSH known-host 等 TauTerm 自有 JSON 状态通过共享原子写入边界提交，新快照完整写入后才替换旧文件。

Tauri command 按阻塞风险分类：纯内存/短锁读取可以同步；文件系统、进程、凭据后端、驱动/平台探测、thread join 等潜在阻塞工作必须使用 async command，并在需要时进入 blocking worker。端点发现同样是配置辅助能力，不属于 Session 生命周期；前端进入配置页时按需请求，后端不得让硬件枚举阻塞 UI。公共 `commands.rs` 只承载协议无关 IPC；协议连接编排、协议专属 DTO/Tauri command 与断开后的协议副作用由各插件 application/commands 模块持有，通过 `PluginRuntime` contribution 接入。插件专属 Tauri command 的实现、DTO 与策略保持在插件目录，唯一注册清单由 `plugins/catalog.rs` 持有；`lib.rs` 的 root invoke handler 只列协议无关命令。共享 Session 编排辅助函数位于 application contribution 层，不反向解释 SSH、TFTP、Network 等字段。

前端每个插件模块只导出惰性的 `PluginDefinition`，导入插件模块不得修改全局状态。`src/plugins/catalog.ts` 是唯一了解完整内建前端插件集合的 composition 模块；`main.tsx` 只调用 `installBuiltinPlugins()`。`PluginRegistry.install()` 在写入 registry 前整批校验插件 ID、能力与 contribution 基本不变量，并拒绝重复注册。应用运行期间不支持动态卸载内建插件，因此不存在与后端生命周期不对称的 `unregisterPlugin()` 假接口。

`plugin-contracts.ts` 只提供不依赖 React、i18n 或 SessionContext 的稳定类型合同，不建立第二套 registry。`PluginManifest.name` 是 Session 类型在新建卡片和配置标题中的 canonical 显示身份，`description` 只描述能力。连接表单可以通过 `PluginRegistration.isConnectionConfigValid(params)` 声明“允许创建/保存 Session”的最低条件；默认参数、提交前参数投影和 endpoint 解析分别由 `defaultConnectionParams`、`prepareConnectionParams` 与 `resolveEndpoint` 贡献。Session 选项默认值只通过 `PluginRegistry.getDefaultSessionOptions/resolveSessionOptions` 解析，公共页面不得再维护第二套 transfer/send-bar 默认策略。插件还可以通过 `persistedConnectionParams`、`reconnectGuard`、`formatSessionError`、`sessionPresentation` 与 `workspace.availability` 分别拥有持久化参数投影、重连前置策略、协议错误展示、会话展示格式和断连工作台可见性。统一 Session/UI 层只消费这些通用 contribution，不按 SSH、TFTP、TRDP 等具体插件 ID 复制同一业务规则。

前端全局动作使用 `ShortcutActionId` 作为 canonical identity。`useKeyboard` 维护唯一的运行时 action registry 与共享 document 键盘监听器；键盘快捷键、命令面板和 Toolbar 只负责选择 Action ID，并通过同一 dispatcher 执行。具体行为只在 action owner 注册一次，不允许各入口再维护平行 `switch`、魔术字符串或“显示了命令但没有执行实现”的占位分支。

## 关键生命周期

```mermaid
stateDiagram-v2
  [*] --> Saved
  Saved --> Connecting
  Connecting --> Connected
  Connecting --> Disconnected
  Connected --> Transferring
  Transferring --> Connected
  Connected --> Disconnected
  Transferring --> Disconnected
  Disconnected --> Connecting
  Disconnected --> [*]
```

`Transferring` 只表示 Session 的 inline 独占资源被传输任务占用；真实底层资源仍归 DataPlane Runtime 所有。连接、断开、异常退出、子连接关闭和传输结束必须通过统一生命周期收敛。`SessionDataPlane` 对需要注册顺序保证的 Session/child 使用 paused attach：先建立订阅与事件线程但阻塞回调，待 SessionStore 注册和 `session-connected`/peer joined 发布完成后再显式 activate；因此数据与断开事件不能抢在权威 Session 状态之前。

## 设计边界

- 核心只拥有可复用机制，不加入 TRDP、SSH、Modbus、串口等协议专属判断；`kernel/` 不允许依赖 `crate::plugins::*`。
- 后端内建插件集合只在 `plugins/catalog.rs` 组合。应用 bootstrap 调用 catalog 建立 runtime 和执行宿主生命周期注入，不直接构造具体 Adapter、访问 SSH/Telnet 等专属状态或注册具体插件命令。
- 协议连接入口由 `PluginRuntime` 中注册的类型化 Session connector contribution 分发；公共 `connect_session` 不按插件 ID `match`。新增内建插件使用已有扩展点时，只新增插件目录、canonical manifest，并在前后端 catalog 各加入一个 definition；不得修改公共 Session presentation/status contract、AppState 或通用连接路由。`connect_session`、端点枚举和保存配置都要求显式 `plugin_id`，公共层不提供 Serial 等具体插件的兼容默认值。
- 插件专属运行态必须由插件对象或 Session capability 持有，不把 SSH known-host verifier、协议 runtime registry 等字段泄漏到 `AppState`。Adapter 需要按 `session_id` 查找专属 runtime 时使用 `SessionRuntimeRegistry<T>`：索引实例由 Adapter 持有且只保存 `Weak<T>`，SessionStore capability graph 仍是唯一强生命周期 owner；禁止模块级 `OnceLock`/静态 runtime registry。
- 前端只保留一个 `PluginRegistry`，插件 definition 本身无副作用。依赖无关的 contract 可以独立成类型模块，但不得为了绕过循环依赖再镜像插件注册数据；会话 presentation、连接参数准备、重连策略、断连工作台可见性、状态栏项和自定义视图均由插件 definition 直接声明。
- 通用前端状态/渲染合同只包含协议无关字段。Serial 虚拟端口、SSH journald/文件服务、Network peer、TRDP A/B 链路等私有运行态不能为了某个 renderer 的便利继续扩张 `StatusBarContext`、通用 presentation helper 或其它公共 registry contract；插件 renderer 应通过自己的 Session/plugin store 获取私有状态。
- UI 能力由插件 manifest/definition 声明；SendBar、自定义视图、连接配置合法性、断连工作台可见性等不由页面临时猜测。公共页面不得通过 built-in plugin ID 分支决定这些行为。
- 插件私有工作台、协议视图和工具组件归 `src/plugins/<id>/` 所有；`src/components/` 只保留真正跨插件共享的 UI 组件。
- 全局动作行为只由 action registry owner 注册一次；Toolbar、命令面板和键盘绑定不得复制行为分发表，也不得使用脱离 `ShortcutActionId` 的第二套 action 名称。
- 运行时对象不能被持久化为 Session 配置；插件需要从编辑态参数剥离凭据或其它瞬态字段时，通过自己的持久化投影 contribution 完成，公共 Session 层不解释字段名。
- 所有单流 Session 都通过 `SessionIo/DataPlane` 发送、订阅和关闭。多路 Container Session 若需要公共 SendBar/脚本，只暴露最小 `AutomationIo/AutomationRx` capability；它不能为了接入公共 UI 把某个子流伪装成根 DataPlane。
- 需要独占主字节流的操作使用 `SessionIo::acquire_exclusive`；不转移底层 handle 所有权。
- PTY resize、targeted send、多 peer、SFTP 等属于独立 capability，不塞进万能 stream trait。文件传输 provider 的具体执行策略只存在于 `transfer/`；Kernel 仅传递不透明传输协议 ID，不维护 X/Y/ZModem、SFTP 等 provider 名称或执行模式。
- 异常断开必须保留足够信息供 UI 呈现，同时后端负责确定性资源清理。
- 不实现运行时第三方插件、动态 Rust ABI、热卸载或兼容 shim；出现明确产品需求前，优先保持显式、可审计、可静态验证的编译期插件组合。

## 代码锚点

- `src-tauri/src/kernel/session_store.rs`
- `src-tauri/src/kernel/plugin_adapter.rs`
- `src-tauri/src/kernel/plugin_runtime.rs`
- `src-tauri/src/plugins/catalog.rs`
- `src-tauri/src/session/io.rs`
- `src-tauri/src/session/runtime.rs`
- `src-tauri/src/transport/runtime.rs`
- `src-tauri/src/kernel/config_store.rs`
- `src-tauri/src/kernel/persistence.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/commands/`
- `src/core/plugin-contracts.ts`
- `src/core/plugin-registry.ts`
- `src/plugins/catalog.ts`
- `src/context/SessionContext.tsx`
- `src/hooks/useKeyboard.ts`
- `src/shortcuts/actionIds.ts`

## 单一来源约束

内建插件静态元数据位于 `src/plugin-manifests/*.json`，TypeScript `PluginDefinition` 与 Rust `PluginRuntime` 都消费这组 canonical manifest。manifest 的 `name`、`description`、`icon`、`content_type`、capabilities 与 transfer protocol 声明不得由公共层二次拼装或用其它字段代替；`ProtocolAdapter` 不再维护第二份 `content_type`。前端 catalog 只叠加 UI/application contribution，重复插件 ID 在批量安装前直接失败；不存在独立的 Session presentation registry。后端 runtime 将 manifest、可选 `ProtocolAdapter` 与类型化 contribution 收敛为单一注册记录，并在启动时校验 Adapter 自声明 ID 与 manifest ID 一致。Session 运行时生命周期仍只由 `SessionStore` 负责。

全局动作的 identity 只来自 `src/shortcuts/actionIds.ts`，运行时行为只来自共享 action registry；快捷键配置、命令面板和 Toolbar 都消费这些 Action ID，不建立第二份行为映射。

分屏/Workspace Layout 的真实 owner 是前端 SplitLayoutContext + `core/split-layout.ts`；不保留未接入运行时的平行 WindowManager/TabHost/IPC 骨架。通用 Tauri invoke/event 是当前 IPC 边界。

架构回归测试不仅检查 Rust import 方向，也检查前端公共 plugin/status/presentation contract、catalog 装配方式、插件私有 UI 归属和默认策略不重新引入具体协议字段或 built-in plugin 分支。插件扩展性属于可执行架构约束，而不是仅靠文档约定。

## 何时更新本文

修改 Session 状态机、插件注册契约、前后端 catalog、公共连接路由、DataPlane/SessionIo 能力、父子会话模型或配置公共职责时，必须同步更新本文。

Transport/DataPlane 细节见 [TRANSPORT_RUNTIME.md](TRANSPORT_RUNTIME.md)；共享文件传输见 [TRANSFER.md](TRANSFER.md)；数据批处理/日志见 [OBSERVABILITY_TOOLS.md](OBSERVABILITY_TOOLS.md)。