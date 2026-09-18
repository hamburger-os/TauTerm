---
name: tauterm-com0com
description: "TauTerm Windows com0com virtual-port architecture and maintenance reference. Use for com0com, setupc.exe, virtual COM pairs, TauTermService, driver install/uninstall, endpoint ownership, orphan recovery, COM allocation, CNCA/CNCB, privilege/service failures, and related Chinese queries such as 虚拟串口、端口对、驱动安装、残留端口、权限不足。"
license: MIT
metadata:
  author: tauterm
  version: "3.4"
---

# TauTerm com0com 虚拟串口维护参考

> 默认使用简体中文输出诊断和修改说明。
>
> 本文是 TauTerm 的 **Windows com0com 实现维护规则**。协议/产品层设计见 `docs/modules/SERIAL.md`；setupc 的原生命令语法见 `references/setupc-cli.md`。不要在多个文档里重复维护同一套实现细节。

## 1. 不变量：先守住产品边界

修改虚拟串口之前先确认以下规则全部成立：

1. **Windows 生产路径优先通过 `TauTermService` 执行 com0com 特权操作。** 服务以 LocalSystem 运行，App 通过窄类型命名管道协议请求固定操作，绝不透传任意 setupc 参数。Release 构建服务不可用时进入 `direct-uac-on-demand`；Debug 开发构建直接使用该模式。direct-UAC 通过当前 TauTerm 可执行文件的窄类型 one-shot helper 执行，GUI/helper 双向校验 pipe PID；普通 GUI 永不执行 `setupc.exe`。普通启动不执行 `setupc list`/orphan cleanup，只有用户明确创建、安装或手动清理时才进入按需 UAC。
2. **用户只看到 external endpoint。** `VirtualEndpoint.bridge_path` 是 TauTerm 内部桥接资源，`external_path` 才是用户和第三方串口工具应该打开的端口。
3. **内部 bridge 必须从普通 Serial 端点发现中隐藏。** Windows 直连后端和 ServiceBackend 客户端都通过 `virtual_port::backend` 的内部端点注册表维护可见性。
4. **前端契约只暴露 `external_path`。** 不把 `bridge_path`、CNCA/CNCB 或 bus 编号泄漏到 UI。状态栏示例：`VPort: COM21`。
5. **删除授权来自 endpoint ownership，不来自驱动枚举。** `setupc list` 可以在特权服务或显式管理员诊断中用于端口/bus 冲突检测和状态核对，但绝不能因为“驱动里存在某个 bus”就推断它属于 TauTerm，也不能为了普通启动诊断而触发提权失败。
6. **严格定义 `orphan = owned_endpoints - active_endpoints`。** active 端点永远不是残留；外部程序关闭 external endpoint 也不会改变父 Serial Session 的 ownership。
7. **第三方 com0com 端口对不可被 TauTerm 清理。** 手动“清理残留端口”只能处理 TauTerm 有 ownership 证据且当前无 active owner 的资源。
8. **服务与 direct-UAC 共用受保护的机器级 endpoint ownership。** 唯一 ledger 位于 `%ProgramData%\TauTerm\virtual-port\com0com_state.json`；Authenticated Users 只读，SYSTEM/Administrators 可写。普通 GUI 不能修改该文件，删除授权只能由特权服务/helper 创建的记录产生。
9. **ownership 记录带 owner PID。** 另一个仍在运行的 TauTerm 实例创建的 endpoint 不能被当前服务/helper 当作 orphan；服务崩溃、App 异常退出后才允许按 protected ledger 恢复。
10. **在线升级保留 endpoint ownership；正式卸载删除 TauTerm 自己的机器级状态。** NSIS hook 负责服务生命周期以及 `%ProgramData%\TauTerm\virtual-port` 与 driver marker 目录清理。
11. **当前 endpoint ownership schema 唯一，不做旧 bus-only 兼容迁移。** 预稳定阶段遇到旧/损坏 schema，只允许特权边界做诊断备份并重置；普通 GUI 不修写机器级状态。
12. **com0com 驱动是系统级共享资源，driver ownership 与 endpoint ownership 完全分离。** 只有安装前确认系统没有 com0com、且本次由 TauTerm 成功安装时，才写 `%ProgramData%\TauTerm\service\driver-owned.marker`。卸载时必须同时满足“driver-owned marker 存在”和“`setupc list` 已确认没有任何端口对”，才允许全局 `setupc uninstall`；否则保留共享驱动。
13. **DataPlane pump 不做 external COM 阻塞 I/O。** 物理 → 虚拟 fan-out 只向每个 endpoint 的独立有界 egress 入队；一个已打开但停止读取的 external peer 只能把自己的 endpoint 标记为 backpressured，不能堵塞 DataPlane 或其它 endpoint。Windows 发生数据完整性缺口后必须观察到 close → reopen 才以 fresh stream 恢复，不补发缺口期间的历史数据。

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
| Windows com0com endpoint ownership、分配、创建、销毁、恢复 | `src-tauri/src/virtual_port/manager.rs` |
| Windows protected ownership 路径与 DACL | `src-tauri/src/virtual_port/windows_state.rs` |
| Windows direct-UAC one-shot helper | `src-tauri/src/virtual_port/elevated.rs` |
| App ↔ 特权服务客户端 | `src-tauri/src/virtual_port/service_backend.rs` |
| Windows LocalSystem 服务 | `src-tauri/src/bin/tauterm-service.rs` |
| Serial Session / VPort 生命周期编排 | `src-tauri/src/plugins/serial/mod.rs` |
| VPort 数据桥接与 endpoint backpressure | `src-tauri/src/virtual_port/bridge.rs` |
| 驱动状态/显式残留清理 Tauri 命令 | `src-tauri/src/commands/platform.rs` |
| Serial 端点发现与展示描述 | `src-tauri/src/plugins/serial/mod.rs` |
| 前端 VPort runtime 状态 | `src/plugins/serial/runtime-store.ts` |
| 前端 Serial 状态栏 | `src/plugins/serial/SerialStatusItems.tsx` |
| 前端驱动/orphan 状态 | `src/hooks/useCom0comStatus.ts` |
| Windows 安装/更新/卸载与 driver ownership | `src-tauri/windows/hooks.nsh` |
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
CNCA<n> -> bridge_path    -> PortName=COMxx,dsr=ropen
CNCB<n> -> external_path  -> PortName=COMyy,PlugInMode=yes
```

`dsr=ropen` 把远端 external endpoint 的打开状态映射到 bridge 端 DSR。Bridge 只在 peer 实际打开时转发 physical → virtual 数据；peer 缺席期间不积压历史数据，peer 已连接后出现的真实 I/O/完整性失败则 fail-closed。bus 是后端资源标识；前端不得依赖它。

### 3.2 setupc 属于特权边界

`install`、`remove`、`change`、`uninstall` 等写操作需要管理员权限。实际部署中某些 `setupc.exe` 构建连 `list` 也可能因执行清单/UAC 策略要求提升，因此产品不能假设“只读命令一定能由普通 GUI 安静执行”。

产品安装后由 `TauTermService` 承担 endpoint 特权操作与启动期 orphan reconciliation；服务不可用时 GUI 只通过 narrow direct-UAC helper 在用户明确动作中按需提权，普通启动阶段不启动 `setupc.exe` 做全局枚举/清理。direct helper 只接收 typed operation，不允许 `.cmd`、PowerShell 或任意 setupc 参数透传。

### 3.3 setupc 必须在资源目录运行

`setupc.exe` 会从工作目录读取配套 INF/SYS/DLL/CAT 文件。调用时必须：

- executable 指向打包后的 `setupc.exe`；
- `current_dir` 设置为 com0com 资源目录；
- 不依赖 PATH；
- Windows 后台调用使用 `CREATE_NO_WINDOW`；
- **产品调用统一带 `--silent`**，com0com 自己的交互式冲突确认窗口不能成为产品控制流。

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

## 4. Endpoint ownership 与特权事务

Windows 唯一机器级 ledger：

```text
%ProgramData%\TauTerm\virtual-port\com0com_state.json
```

其目录 DACL 为 protected：Authenticated Users 只读，SYSTEM/Administrators 完全控制。GUI 读取它来隐藏内部 bridge、计算本进程视角的 orphan；**只有 TauTermService/direct-UAC helper 能写**。这条边界不能退化回用户可写 AppData，否则普通用户可以伪造 ownership 再诱导特权 cleanup 删除第三方 bus。

记录包含完整 endpoint 与 owner PID。回收条件是：

```text
reclaimable = owned
            - 当前 backend active
            - other TauTerm process still alive
```

### 4.1 创建必须在特权事务内决定真实 bus/COM

禁止“普通 GUI 先猜 bus → 预写 ownership → 提权后执行 install”。正确流程：

1. 特权 service/helper 获取全局 `Global\TauTermCom0comMutation` mutex；
2. 特权上下文执行 `setupc --silent list`，并结合 `serialport::available_ports()` 与 protected ledger 建立权威冲突状态；
3. 在产品区间选择空闲 bus/COM，跳过测试预留区 `200..=255`；
4. 执行 `setupc --silent install`；
5. 再次 `list`，核验实际 CNCA/CNCB 与请求的 bridge/external COM 映射；
6. 以**实际 bus**提交 protected ownership，再将 endpoint 交给当前 Session/backend active 集；
7. 批量创建任一环节失败，回滚本批已验证创建的资源。

如果 com0com 想把请求的 `CNCA0/CNCB0` 自动改成另一 bus，产品不能让用户点击“继续”后继续按旧 bus 记账。silent + post-install verification 必须把这种情况变成 TauTerm 自己可判定的成功/失败。

### 4.2 正常销毁与 direct-UAC 延迟回收

特权路径优先：

```text
setupc --silent remove <bus>
```

若端口仍被占用，可解绑 COM 名称后有限重试。成功后从 protected ledger 与内部 bridge registry 移除。

direct-UAC Session 正常断开不弹第二次 UAC。普通 GUI 只结束本地 active/hide 状态，protected ownership 继续保留；下一次明确 create/manual cleanup 时 helper 在同一次 UAC 事务里先回收。因为 service 与 helper 共用 ledger，之后恢复正常的 TauTermService 也能识别这些 direct-UAC 记录；仍有 live owner PID 的记录必须跳过。

### 4.3 崩溃、旧 schema 与第三方资源

- App/service 崩溃后，protected record 保留；只有 owner 已不再运行且当前 backend 不 active 时才成为可回收 orphan；
- 驱动中不存在的 owned bus 只删除 ledger 记录，不反复 remove；
- 当前 schema 不兼容旧 bus-only 状态。旧/损坏记录只允许特权进程备份并重置，GUI 不写；
- `setupc list` 是冲突/存在性事实，不是删除授权。未知 bus 永远视为第三方/不可证明资源；
- driver ownership 与 endpoint ownership 独立，不能用 endpoint ledger 授权全局 uninstall。

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
- `client_id` 只表示当前管道连接的运行期资源归属，机器级 crash recovery 仍依赖持久化 endpoint ownership。

不要把“驱动里可枚举到的 bus”加入服务 endpoint ownership；ownership 只能由 TauTerm 自己的创建路径产生。

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

正常安装应存在并运行。服务不可用时优先检查安装/升级流程和 `tauterm-service.exe`，不要先在 UI 增加另一套提权实现。开发态服务不可用可以进入 direct UAC fallback，但普通启动不应因此出现 `setupc list` 的 740 日志。

### 7.2 检查 com0com 驱动

```cmd
sc query com0com
```

需要进一步核对端口对时，在管理员终端进入 com0com 资源目录执行：

```cmd
setupc.exe --silent list
```

把 setupc 枚举视为管理员/服务侧诊断能力，不作为普通 GUI 启动探针。

### 7.3 检查资源文件

确认 7 个必需文件全部存在于打包资源目录。

### 7.4 检查 endpoint ownership

生产服务状态：

```text
%ProgramData%\TauTerm\virtual-port\com0com_state.json
```

判断残留时永远使用：

```text
owned - active
```

不能把 `setupc list` 的全部 bus 数量当作 `orphan_count`。

### 7.5 检查 driver ownership

TauTerm 只有在“安装前 com0com 不存在、安装动作成功”时才创建：

```text
%ProgramData%\TauTerm\service\driver-owned.marker
```

marker 仅表示 **TauTerm 对系统级驱动安装拥有卸载资格的必要条件之一**，并不意味着可以无条件卸载。真正卸载前还必须确认 `setupc list` 中没有任何端口对。

### 7.6 检查可见性

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

普通 GUI 启动阶段不应主动执行 `setupc list`，因此“服务不可用 + 启动即 740”属于权限边界实现错误，而不是正常噪声。开发环境确需直接运行 setupc 时，可手工以管理员终端执行；这属于诊断/开发流程，不是产品 UI 的第二套权限模型。

### 8.2 `PortName in use` / `already logged` / `already exists`

Windows COM 名数据库可能仍占用某个端口号，即使普通枚举看不到。创建逻辑应跳过该候选继续扫描，而不是复用一个来源不明的现存 bus。

需要人工诊断时可：

```cmd
setupc.exe --silent busynames COM*
```

### 8.3 `remove` 非零但资源已经不存在

不要仅凭 exit code 推断资源存在。由特权服务或管理员诊断用 `setupc list` 核对目标 bus；若已经不存在，应清掉 TauTerm 对应 endpoint ownership 记录，而不是反复删除。

### 8.4 外部 COM 断开后状态栏出现“残留端口”

这是错误行为。external consumer 的打开/关闭不改变父 Session ownership。只要父 Session active：

```text
endpoint ∈ owned
endpoint ∈ active
=> endpoint ∉ orphan
```

### 8.5 服务崩溃后留下端口

重启服务应根据受保护的 ProgramData endpoint ownership 自动恢复/清理；direct-UAC 遗留也使用同一本 ledger。若 orphan 仍存在，检查：

- ownership 文件是否存在且可读；
- `setupc list` 是否仍能看到 bus；
- external 端口是否仍被第三方进程占用；
- 服务日志中的 cleanup 失败原因。

不要“修复”为删除驱动中所有 com0com bus。

### 8.6 卸载后 com0com 驱动仍存在

这不一定是残留错误。若以下任一条件成立，TauTerm 应主动保留共享驱动：

- 安装 TauTerm 前 com0com 已存在；
- `driver-owned.marker` 不存在；
- `setupc list` 仍有任意端口对；
- 无法可靠确认驱动当前端口状态。

只有 marker 存在且驱动中已经没有任何端口对时，才允许 `setupc uninstall`。

---

## 9. 安装、更新与卸载

NSIS 规则：

- **安装**：安装前先查询 `sc query com0com`。若驱动原本不存在且 TauTerm 成功通过临时端口对装入驱动，创建 `driver-owned.marker`；若驱动原本存在，不取得 driver ownership；
- **在线升级**：先停止/结束旧 App 与服务以释放文件锁，保留 `%ProgramData%\TauTerm\virtual-port` endpoint ownership 和 driver marker，新服务启动后继续恢复；
- **卸载**：先结束 GUI，让服务处理客户端管道断开；再优雅停止服务并以强杀兜底。只有 driver marker 存在且 `setupc list` 已确认没有任何端口对时，才卸载 com0com；否则保留共享驱动。随后删除 TauTerm 自己的 `%ProgramData%\TauTerm\virtual-port` ownership、`%ProgramData%\TauTerm\service` driver marker 状态和安装目录；
- **禁止**：因为 TauTerm 曾使用 com0com 就无条件执行全局 `setupc uninstall`。全局驱动和 endpoint ownership 是两个不同资源层级；
- 不用“重启后删除”掩盖安装目录锁问题。

修改 `src-tauri/windows/hooks.nsh` 后必须在 Windows CI/安装包实测：安装、已有第三方 com0com 的安装、在线更新、正常卸载、存在第三方端口对时卸载五条路径都要覆盖。

---

## 10. 验证矩阵

至少覆盖：

| 场景 | 期望 |
|---|---|
| 普通物理串口发现 | 名称不重复，identity 保留 |
| 创建 1 对虚拟端口 | bridge 隐藏，external 可见/可用 |
| 创建多对 | 特权事务内分配 bus/COM，无交互式 setupc 窗口，安装后实际映射全部验证并有 protected ownership |
| 第三方打开/关闭 external | 不产生 orphan，可重新打开 |
| external 已打开但停止读取 | 仅该 endpoint 进入 backpressured；DataPlane、父 Serial 与其它 endpoint 继续运行 |
| backpressured external 关闭后重新打开 | endpoint 以 fresh stream 恢复，不补发缺口期间历史数据 |
| Debug 构建启动 | 直接进入 direct-uac-on-demand，不探测正式 TauTermService，不产生预期身份拒绝 WARN |
| 父 Serial Session 断开 | 只清理该 Session 的端点 |
| App 崩溃 | 服务检测管道断开并清理该 client |
| Service 崩溃/掉电 | 重启依据 protected ProgramData endpoint ownership 恢复 |
| Service 不可用、普通 GUI 启动 | 进入 direct-uac-on-demand；GUI 不执行 setupc、不弹 UAC、不产生 740；明确创建时只出现 TauTerm helper 的 UAC |
| CNCA/CNCB bus 已占用 | 无 com0com 交互确认框；特权事务重新选择/验证真实 bus，不按请求 bus 错记 ownership |
| 部分创建失败 | 已创建的新资源回滚，不产生未知资源或猜测 ownership |
| 手动 cleanup | 不删除 active、不删除第三方 bus |
| 系统已有第三方 com0com 后安装 TauTerm | 不创建 driver marker，卸载时不碰共享驱动 |
| TauTerm 自己安装 com0com，驱动中无任何端口对 | 卸载时允许删除驱动 |
| TauTerm 自己安装 com0com，但仍有第三方端口对 | 卸载时保留驱动 |
| 在线升级 | endpoint ownership 与 driver marker 保留，新服务恢复 |
| 正式卸载 | 服务和 TauTerm 自己的机器级状态清理；共享驱动按 ownership 边界决定是否保留 |
| reserved test region | 产品不分配 COM/bus 200..=255 |

Windows 真实 com0com 驱动回归不能完全由跨平台单元测试替代。合并前至少保证仓库 CI、Windows Rust checks、Runtime E2E 全绿；发布前再做真实驱动链路验证。

---

## 11. setupc 命令速查

完整语法以 `references/setupc-cli.md` 为准。维护实现时最常用：

```text
setupc.exe --silent list
setupc.exe --silent busynames COM*
setupc.exe --silent install <bus> PortName=COMxx PortName=COMyy,PlugInMode=yes
setupc.exe --silent change CNCA<bus> PortName=-
setupc.exe --silent change CNCB<bus> PortName=-
setupc.exe --silent remove <bus>
setupc.exe --silent uninstall
```

所有写操作必须在明确的权限边界内执行；产品代码优先通过 TauTermService，不要把这些命令直接暴露给 WebView。`setupc list` 的普通启动探测同样禁止；需要枚举时使用服务/管理员上下文。`setupc uninstall` 是全局操作，必须额外满足 driver ownership + 无端口对条件。

---

## 12. 平台说明

com0com 只用于 Windows。Linux/macOS 的 TauTerm 虚拟串口能力使用进程内 POSIX PTY 后端，不依赖 socat、Homebrew、PATH 或 `/tmp` 符号链接。不要把 Windows 的 bus/CNCA/CNCB 语义扩散到 Unix 后端或前端。