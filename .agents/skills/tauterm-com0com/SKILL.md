---
name: tauterm-com0com
description: "TauTerm Windows com0com virtual-port architecture and maintenance reference. Use for com0com, setupc.exe, virtual COM pairs, TauTermService, driver install/uninstall, endpoint ownership, orphan recovery, COM allocation, CNCA/CNCB, privilege/service failures, and related Chinese queries such as 虚拟串口、端口对、驱动安装、残留端口、权限不足。"
license: MIT
metadata:
  author: tauterm
  version: "3.0"
---

# TauTerm com0com 虚拟串口维护参考

> 默认使用简体中文输出诊断和修改说明。
>
> 本文是 TauTerm 的 **Windows com0com 实现维护规则**。协议/产品层设计见 `docs/modules/SERIAL.md`；setupc 的原生命令语法见 `references/setupc-cli.md`。不要在多个文档里重复维护同一套实现细节。

## 1. 不变量：先守住产品边界

修改虚拟串口之前先确认以下规则全部成立：

1. **Windows 生产路径优先通过 `TauTermService` 执行 com0com 特权操作。** 服务以 LocalSystem 运行，App 通过窄类型命名管道协议请求固定操作，绝不透传任意 setupc 参数。
2. **用户只看到 external endpoint。** `VirtualEndpoint.bridge_path` 是 TauTerm 内部桥接资源，`external_path` 才是用户和第三方串口工具应该打开的端口。
3. **内部 bridge 必须从普通 Serial 端点发现中隐藏。** Windows 直连后端和 ServiceBackend 客户端都通过 `virtual_port::backend` 的内部端点注册表维护可见性。
4. **前端契约只暴露 `external_path`。** 不把 `bridge_path`、CNCA/CNCB 或 bus 编号泄漏到 UI。状态栏示例：`VPort: COM21`。
5. **删除授权来自 ownership，不来自驱动枚举。** `setupc list` 可以用于端口/bus 冲突检测和状态核对，但绝不能因为“驱动里存在某个 bus”就推断它属于 TauTerm。
6. **严格定义 `orphan = owned_endpoints - active_endpoints`。** active 端点永远不是残留；外部程序关闭 external endpoint 也不会改变父 Serial Session 的 ownership。
7. **第三方 com0com 端口对不可被 TauTerm 清理。** 手动“清理残留端口”只能处理 TauTerm 有 ownership 证据且当前无 active owner 的资源。
8. **服务自身崩溃也不能丢失 ownership。** `TauTermService` 的机器级 ownership 持久化在 `%ProgramData%\TauTerm\service\com0com_state.json`。服务重启只恢复/清理这些有证据的 orphan。
9. **在线升级保留服务 ownership；正式卸载删除它。** NSIS hook 负责服务生命周期、com0com 驱动卸载以及 `%ProgramData%\TauTerm\service` 清理。
10. **当前 schema 唯一，不做旧 bus-only 兼容迁移。** 预稳定阶段遇到旧/损坏 schema，只做诊断备份并重新建立当前模型。

典型数据流：

```text
物理 COM6
   ⇅
TauTerm Serial Session
   ⇅
内部 COM20 (bridge_path，仅 TauTerm 打开)
   ⇅
com0com bus
   ⇅
对外 COM21 (external_path，用户/第三方工具打开)
```

用户在 TauTerm 普通串口列表中应看到 COM6 和可用的外部 COM21，不应看到 TauTerm 自己占用的内部 COM20。

---

## 2. 代码单一来源

核心代码位置：

| 责任 | 文件 |
|---|---|
| 平台无关后端接口、`VirtualEndpoint`、内部端点可见性注册表 | `src-tauri/src/virtual_port/backend.rs` |
| Windows com0com ownership、分配、创建、销毁、恢复 | `src-tauri/src/virtual_port/manager.rs` |
| App ↔ 特权服务客户端 | `src-tauri/src/virtual_port/service_backend.rs` |
| Windows LocalSystem 服务 | `src-tauri/src/bin/tauterm-service.rs` |
| Session 创建/桥接生命周期 | `src-tauri/src/commands.rs` |
| 驱动状态/显式残留清理 Tauri 命令 | `src-tauri/src/commands/platform.rs` |
| Serial 端点发现与展示描述 | `src-tauri/src/plugins/serial/mod.rs` |
| 前端状态栏 | `src/components/Layout/StatusBar.tsx` |
| 前端驱动/orphan 状态 | `src/hooks/useCom0comStatus.ts` |
| Windows 安装/更新/卸载 | `src-tauri/windows/hooks.nsh` |
| 产品设计 | `docs/modules/SERIAL.md` |

修改这些边界时同步更新本文和 `docs/modules/SERIAL.md`，不要再建立第二套 ownership、清理或可见性逻辑。

---

## 3. com0com 基础事实

### 3.1 端口对

每个 com0com bus 包含两端：

```text
CNCA<n>  ⇄  CNCB<n>
COM20       COM21
```

TauTerm 当前创建时约定：

```text
CNCA<n> -> bridge_path   -> PortName=COMxx
CNCB<n> -> external_path -> PortName=COMyy,PlugInMode=yes
```

bus 是后端资源标识；前端不得依赖它。

### 3.2 写操作需要管理员权限

`install`、`remove`、`change`、`uninstall` 等写操作需要管理员权限。产品安装后由 `TauTermService` 承担这些特权操作；不要在 UI 层直接拼接 PowerShell/setupc 命令。

### 3.3 setupc 必须在资源目录运行

`setupc.exe` 会从工作目录读取配套 INF/SYS/DLL/CAT 文件。调用时必须：

- executable 指向打包后的 `setupc.exe`；
- `current_dir` 设置为 com0com 资源目录；
- 不依赖 PATH；
- Windows 后台调用使用 `CREATE_NO_WINDOW`。

### 3.4 7 个必需文件

| 文件 | 作用 |
|---|---|
| `setupc.exe` | CLI 管理入口 |
| `setup.dll` | setupc 运行时依赖 |
| `com0com.sys` | 内核驱动 |
| `com0com.inf` | 驱动安装信息 |
| `com0com.cat` | 签名目录 |
| `cncport.inf` | com0com 端口配置 |
| `comport.inf` | COM 端口行为配置 |

`VirtualPortManager::are_files_present()` 与安装打包清单必须保持一致。

---

## 4. Ownership 模型

Windows 有状态后端维护：

```text
active_endpoints  = 当前进程/服务中仍由活动 Session 持有的 endpoint
owned_endpoints   = TauTerm 已创建且仍负责回收的 endpoint（持久化）
orphan_endpoints  = owned_endpoints - active_endpoints
```

### 创建

创建前必须先做冲突检查：

- `serialport::available_ports()`：Windows 当前可枚举 COM；
- `setupc list`：com0com 自身已注册的端口与 bus；
- ownership：TauTerm 已知但当前可能不可枚举的端口。

端口从产品区间扫描，同时跳过测试预留区：

```text
COM 200..=255   测试预留
bus 200..=255   测试预留
```

创建成功后：

1. 持久化 ownership；
2. 注册 `bridge_path` 为内部不可见；
3. 加入 active；
4. 向上层只投影 `external_path`。

批量/提权路径必须是事务式的：发生部分失败时回滚已经创建的本批资源；如果子进程超时或异常终止，仍必须保留 ownership 证据，不能产生“驱动里存在但 TauTerm 完全不知道”的资源。

### 正常销毁

优先：

```text
setupc remove <bus>
```

若端口仍被占用：

```text
setupc change CNCA<bus> PortName=-
setupc change CNCB<bus> PortName=-
等待短暂传播
setupc remove <bus>
```

销毁成功后同时：

- 从 active 移除；
- 从 owned 移除；
- 注销内部 bridge 可见性记录。

销毁暂时失败时：

- active owner 结束；
- ownership 保留；
- 资源成为 orphan；
- 后续通过显式清理或服务重启恢复，不在 Session 断开回调中突然弹权限提示。

### 服务崩溃/掉电恢复

`TauTermService` 使用 `%ProgramData%\TauTerm\service` 的持久化 ownership。服务启动时：

1. 载入 owned；
2. 当前没有旧进程 active owner，因此这些记录是候选 orphan；
3. 用 `setupc list` 核对资源是否仍存在；
4. 已不存在的记录只删除 ownership；
5. 仍存在且确属 TauTerm ownership 的资源尝试清理；
6. 不扫描删除未知 bus。

这是“恢复自己资源”和“全局扫驱动”的关键区别。

---

## 5. 特权服务安全边界

命名管道：

```text
\\.\pipe\TauTermService
```

服务端必须同时满足：

- 管道 ACL 只给已认证用户连接权限，SYSTEM/Administrators 完全控制；
- 连接后读取客户端 PID；
- 客户端文件名必须是 `tauterm.exe`；
- 客户端与服务可执行文件必须位于同一安装目录；
- 只接受固定 op：hello/status/install/create/remove/cleanup 等窄操作；
- 不接受任意 setupc 参数、shell 文本或命令行透传；
- `client_id` 只表示当前管道连接的运行期资源归属，机器级 crash recovery 仍依赖持久化 ownership。

不要把“驱动里可枚举到的 bus”加入服务 ownership；ownership 只能由 TauTerm 自己的创建路径产生。

---

## 6. Serial 端点展示契约

`EndpointInfo` 约定：

```text
name        = 真正用于连接的系统端点，如 COM6
description = 只包含补充说明，不重复 name
params      = 结构化 device identity
```

USB identity 至少保留：

- VID/PID；
- serial number（如果设备提供）；
- manufacturer/product；
- stable_id（只有具备足够稳定信息时才生成）。

驱动友好名若以与当前端口完全匹配的 `(COMx)` 结尾，只移除这一段重复后缀；不要用宽松字符串替换破坏产品名。

发现结果还必须过滤 `is_internal_endpoint_path(port_name)`，保证 TauTerm 自己打开的 bridge 不重新出现在串口选择列表。

---

## 7. 快速诊断流程

### 7.1 检查特权服务

```cmd
sc query TauTermService
```

正常安装应存在并运行。服务不可用时优先检查安装/升级流程和 `tauterm-service.exe`，不要先在 UI 增加另一套提权实现。

### 7.2 检查 com0com 驱动

```cmd
sc query com0com
```

再在 com0com 资源目录执行：

```cmd
setupc.exe list
```

只读枚举可以用于诊断和冲突检查。

### 7.3 检查资源文件

确认 7 个必需文件全部存在于打包资源目录。

### 7.4 检查 ownership

生产服务状态：

```text
%ProgramData%\TauTerm\service\com0com_state.json
```

判断残留时永远使用：

```text
owned - active
```

不能把 `setupc list` 的全部 bus 数量当作 `orphan_count`。

### 7.5 检查可见性

假设物理端口 COM6，TauTerm 创建 COM20/COM21：

- TauTerm Serial 列表：不能出现内部 COM20；
- 状态栏：只显示 `VPort: COM21`；
- 第三方工具：连接 COM21；
- 断开第三方工具后重新打开 COM21：父 Session 未结束时应仍可用；
- 断开 COM6 父 Session 后：端口对应被销毁或进入明确 orphan 状态。

---

## 8. 常见问题

### 8.1 `os error 740` / Access denied

产品安装场景先检查 `TauTermService` 是否正常。不要默认通过前端 PowerShell `RunAs` 绕过服务架构。

开发环境确需直接运行 setupc 时，可手工以管理员终端执行；这属于诊断/开发流程，不是产品 UI 的第二套权限模型。

### 8.2 `PortName in use` / `already logged` / `already exists`

Windows COM 名数据库可能仍占用某个端口号，即使普通枚举看不到。创建逻辑应跳过该候选继续扫描，而不是复用一个来源不明的现存 bus。

需要人工诊断时可：

```cmd
setupc.exe busynames COM*
```

### 8.3 `remove` 非零但资源已经不存在

不要仅凭 exit code 推断资源存在。用 `setupc list` 核对目标 bus；若已经不存在，应清掉 TauTerm 对应 ownership 记录，而不是反复删除。

### 8.4 外部 COM 断开后状态栏出现“残留端口”

这是错误行为。external consumer 的打开/关闭不改变父 Session ownership。只要父 Session active：

```text
endpoint ∈ owned
endpoint ∈ active
=> endpoint ∉ orphan
```

### 8.5 服务崩溃后留下端口

重启服务应根据 ProgramData ownership 自动恢复/清理。若 orphan 仍存在，检查：

- ownership 文件是否存在且可读；
- `setupc list` 是否仍能看到 bus；
- external 端口是否仍被第三方进程占用；
- 服务日志中的 cleanup 失败原因。

不要“修复”为删除驱动中所有 com0com bus。

---

## 9. 安装、更新与卸载

NSIS 规则：

- 安装：使用随包 setupc 触发驱动安装，并注册 `TauTermService`；
- 在线升级：先停止/结束旧 App 与服务以释放文件锁，**保留 ProgramData ownership**，新服务启动后继续恢复；
- 卸载：停止并删除服务、卸载 com0com 驱动、删除 `%ProgramData%\TauTerm\service`，最后同步清理安装目录；
- 不用“重启后删除”掩盖安装目录锁问题。

修改 `src-tauri/windows/hooks.nsh` 后必须在 Windows CI/安装包实测：安装、在线更新、卸载三条路径都要覆盖。

---

## 10. 验证矩阵

至少覆盖：

| 场景 | 期望 |
|---|---|
| 普通物理串口发现 | 名称不重复，identity 保留 |
| 创建 1 对虚拟端口 | bridge 隐藏，external 可见/可用 |
| 创建多对 | bus/COM 不冲突，全部 ownership 明确 |
| 第三方打开/关闭 external | 不产生 orphan，可重新打开 |
| 父 Serial Session 断开 | 只清理该 Session 的端点 |
| App 崩溃 | 服务检测管道断开并清理该 client |
| Service 崩溃/掉电 | 重启依据 ProgramData ownership 恢复 |
| 部分创建失败 | 已创建的新资源回滚或保留明确 ownership，不产生未知资源 |
| 手动 cleanup | 不删除 active、不删除第三方 bus |
| 在线升级 | ownership 保留，新服务恢复 |
| 正式卸载 | 服务、驱动、机器级 service state 清理 |
| reserved test region | 产品不分配 COM/bus 200..=255 |

Windows 真实 com0com 驱动回归不能完全由跨平台单元测试替代。合并前至少保证仓库 CI、Windows Rust checks、Runtime E2E 全绿；发布前再做真实驱动链路验证。

---

## 11. setupc 命令速查

完整语法以 `references/setupc-cli.md` 为准。维护实现时最常用：

```text
setupc.exe list
setupc.exe busynames COM*
setupc.exe install <bus> PortName=COMxx PortName=COMyy,PlugInMode=yes
setupc.exe change CNCA<bus> PortName=-
setupc.exe change CNCB<bus> PortName=-
setupc.exe remove <bus>
setupc.exe uninstall
```

所有写操作必须在明确的权限边界内执行；产品代码优先通过 TauTermService，不要把这些命令直接暴露给 WebView。

---

## 12. 平台说明

com0com 只用于 Windows。Linux/macOS 的 TauTerm 虚拟串口能力使用进程内 POSIX PTY 后端，不依赖 socat、Homebrew、PATH 或 `/tmp` 符号链接。不要把 Windows 的 bus/CNCA/CNCB 语义扩散到 Unix 后端或前端。