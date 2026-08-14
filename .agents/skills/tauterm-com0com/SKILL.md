---
name: tauterm-com0com
description: >
  com0com virtual serial port driver reference for Windows. Use this skill whenever the user asks about com0com, setupc.exe, virtual COM port pairs, driver installation/uninstallation, UAC/elevation issues (error 740), ghost port cleanup, orphan port recovery, CNCA/CNCB port management. Triggers on Chinese queries too: com0com 驱动, 虚拟串口, 端口对, 驱动安装, 权限不足, setupc 命令, COM 端口对, CNCA, CNCB. Covers the full lifecycle: driver files (7 required), setupc CLI, port pair creation/deletion, UAC elevation patterns, and troubleshooting common issues.
license: MIT
metadata:
  author: tauterm
  version: "2.1"
---

# com0com 虚拟串口驱动参考

> **语言偏好**：所有诊断报告、操作指导和故障排查输出默认使用中文（简体中文）。

## 概述

**com0com** 是由 Vyacheslav Frolov 开发的开源（GPL）Windows 内核驱动，用于创建**虚拟 COM 端口对**。端口对的两端通过内核虚拟 null-modem 电缆连接，一端收到的数据会出现在另一端，实现虚拟串口通信。

目前广泛使用的版本为 **com0com v3.0.0.0**（签名版本，支持 Windows 10/11 x64/x86）。

### 端口对架构

```
CNCA0 (Port A) ═══════════════ CNCB0 (Port B)
   COM22                            COM23

CNCA1 (Port A) ═══════════════ CNCB1 (Port B)
   COM24                            COM25
```

每个端口对由总线号（`bus number`，整数）标识，包含两个端口：
- **CNCA&lt;n&gt;**：端口 A（通常由主应用程序打开）
- **CNCB&lt;n&gt;**：端口 B（通常由外部工具打开，常用 `PlugInMode=yes`）

### 管理员权限要求

com0com 的所有写操作（驱动安装、端口对创建/删除、参数修改）需要**管理员权限**。只读操作（`list`、`busynames`）不需要。

### ⚠️ 关键：setupc.exe 必须在包含配套文件的目录中运行

`setupc.exe` 启动时会从**当前工作目录**查找 `com0com.inf`、`cncport.inf`、`comport.inf`、`com0com.sys`、`com0com.cat`、`setup.dll` 六个配套文件。如果从其他目录直接调用（如 `C:\tools\setupc.exe uninstall`），会报错：

```
SetupOpenInfFile(...\com0com.inf) ERROR: 2 - The system cannot find the file specified.
```

**正确的执行方式**（所有手动命令均遵循此模式）：
```bash
# 先切换到 setupc.exe 所在目录，再执行命令
pushd <setupc_dir> && ./setupc.exe <command> [args...]; popd

# 或在脚本中
cd <setupc_dir> && ./setupc.exe <command> [args...]
```

### 7 个必需文件

| # | 文件 | 角色 | 缺失时的后果 |
|---|------|------|-------------|
| 1 | `setupc.exe` | CLI 管理入口 | 无法执行任何 com0com 操作 |
| 2 | `setup.dll` | setupc.exe 运行时依赖 | setupc.exe 启动报错：找不到 SETUP.dll |
| 3 | `com0com.sys` | 内核驱动 | 驱动安装失败 |
| 4 | `com0com.inf` | 驱动安装信息 | 驱动安装失败 |
| 5 | `com0com.cat` | 驱动签名安全目录 | 签名验证失败 |
| 6 | `cncport.inf` | COM 端口配置 | `SetupOpenInfFile` 错误 |
| 7 | `comport.inf` | COM 端口行为配置 | 端口行为异常或安装失败 |

这 7 个文件必须全部存在于同一目录中，且与目标系统架构匹配（x64 或 x86）。

> **文件来源**：从 [com0com SourceForge 页面](https://sourceforge.net/projects/com0com/) 下载官方签名安装包，用 7-Zip 提取文件。

---

## 工作流 A：快速诊断

当用户问 "com0com 是否正常？"、"虚拟串口为什么没出现？" 时，按以下流程执行诊断：

### Step 1: 定位 setupc.exe

确定 `setupc.exe` 所在目录（设为 `$SETUPC_DIR`）。

### Step 2: 检查 7 个必需文件

```bash
SETUPC_DIR="<path-to-com0com-dir>"
for f in setupc.exe setup.dll com0com.sys com0com.inf com0com.cat cncport.inf comport.inf; do
  [ -f "$SETUPC_DIR/$f" ] && echo "✓ $f" || echo "✗ $f — 缺失！"
done
```

### Step 3: 检测驱动状态

```bash
# 方法 1: Windows 服务查询（推荐）
sc query com0com

# 方法 2: setupc list（需要文件完整）
pushd "$SETUPC_DIR" && ./setupc.exe list; popd
```

- `sc query` 返回 `RUNNING` → 驱动已安装且运行中
- `sc query` 返回 `FAILED 1060`（服务不存在） → 驱动未安装
- `setupc list` 有输出 → 至少有端口对或驱动已加载

### Step 4: 列出当前端口对

```bash
pushd "$SETUPC_DIR" && ./setupc.exe list; popd
# 输出示例:
# CNCA0 PortName=COM22
# CNCB0 PortName=COM23
```

### Step 5: 检查系统日志

```bash
# 检查 com0com 服务状态
sc query com0com

# 检查 Windows 事件查看器中的系统事件
# 搜索来源为 "com0com" 的事件
```

### Step 6: 输出诊断报告

汇总以上检查结果，以结构化格式输出：

```markdown
## com0com 诊断报告

| 检查项 | 状态 | 详情 |
|--------|------|------|
| 驱动文件完整性 | ✓ / ✗ | 缺失: <列出缺失文件> |
| 驱动安装状态 | ✓ 已安装 / ✗ 未安装 | sc query 结果 |
| 活跃端口对 | N 对 | <列出端口对> |
| 管理员权限 | ✓ / ✗ | 当前进程是否以管理员运行 |

**建议操作**: <根据诊断结果给出下一步>
```

---

## 工作流 B：安装驱动

### 前置条件

1. 7 个必需文件完整（参考工作流 A Step 2）
2. 管理员权限（当前进程已提权，或可触发 UAC 提权）

### 安装步骤

**推荐方法（创建临时端口对触发驱动安装）：**
```bash
SETUPC_DIR="<path-to-com0com-dir>"

# 在总线 0 上创建一个临时端口对（携带默认参数），
# 这会触发 Windows PnP 管理器安装 com0com 内核驱动
pushd "$SETUPC_DIR" && ./setupc.exe install 0 - -; popd

# 立即删除临时端口对，驱动保留在系统中
pushd "$SETUPC_DIR" && ./setupc.exe remove 0; popd
```

> **原理**：com0com 驱动在第一次 `install` 命令时自动注册到 Windows。通过创建并立即删除一个临时端口对，驱动被安装但不留下端口对。

### 验证安装

```bash
sc query com0com                        # 应显示 STATE: 4 RUNNING
pushd "$SETUPC_DIR" && ./setupc.exe list; popd  # 应正常执行（即使输出为空）
```

### UAC 提权（权限不足时）

如果当前进程没有管理员权限（os error 740 / ELEVATION_REQUIRED），通过 PowerShell 触发 UAC 提权：

```powershell
# 在提权环境中执行 setupc（注意: cd 到配套文件目录）
Start-Process powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList '-NoProfile','-Command','cd ''<path-to-com0com-dir>''; & ''.\setupc.exe'' install 0 - -; & ''.\setupc.exe'' remove 0'
```

也可以在应用安装包中集成此步骤，在安装阶段自动完成驱动注册。

---

## 工作流 C：创建虚拟端口对

### 前置条件

1. 驱动已安装（工作流 B）
2. 管理员权限

### 扫描可用 COM 端口号

```bash
# 列出当前系统占用的 COM 端口名
pushd "$SETUPC_DIR" && ./setupc.exe busynames COM*; popd
```

系统级 COM 端口枚举可通过 Windows API (`SetupDiGetClassDevs`) 或 `serialport` 库实现。

### 创建端口对

```bash
SETUPC_DIR="<path-to-com0com-dir>"

# 基本命令格式
pushd "$SETUPC_DIR" && ./setupc.exe install <bus> <portA_params> <portB_params>; popd

# 示例 1: 使用默认参数
pushd "$SETUPC_DIR" && ./setupc.exe install 0 - -; popd

# 示例 2: 指定 COM 端口号，B 端口启用 PlugIn 模式
pushd "$SETUPC_DIR" && ./setupc.exe install 0 PortName=COM22 "PortName=COM23,PlugInMode=yes"; popd

# 示例 3: 启用波特率模拟 + 超限模拟
pushd "$SETUPC_DIR" && ./setupc.exe install 0 \
  "PortName=COM22,EmuBR=yes,EmuOverrun=yes" \
  "PortName=COM23,EmuBR=yes,PlugInMode=yes"; popd
```

> **PlugInMode=yes** 的作用：B 端口在未被外部工具打开时自动隐藏。这可防止外部工具意外打开"空"端口，同时确保端口数量与实际需求匹配。

### 预查询 com0com 内部状态

```bash
# 查询驱动中已注册的端口名和 bus 号（只读，无需管理员）
pushd "$SETUPC_DIR" && ./setupc.exe list; popd
# 输出示例:
# CNCA0 PortName=COM22
# CNCB0 PortName=COM23,PlugInMode=yes
```

> **建议**：创建端口前先执行 `setupc list`，将已注册的 COM 端口号加入排除列表，并从最大 bus 号 +1 开始分配新端口，避免与残留端口对冲突。

### 处理常见错误

| 错误信息 | 原因 | 处理方式 |
|----------|------|----------|
| `PortName in use` / `already logged` / `already exists` | COM 端口号被 Windows COM 端口数据库标记为"已占用"（幽灵设备） | 跳过该端口，使用下一个候选端口号。详见工作流 E3「幽灵端口清理」 |
| `os error 740` / `ELEVATION_REQUIRED` | 权限不足 | 触发 UAC 提权（工作流 B — UAC 提权） |
| `exit code 1`（端口名冲突） | 同名端口对已存在 | 可直接复用已有端口对，或先删除再创建 |

---

## 工作流 D：清理与卸载

### 删除单个端口对（两阶段清理策略）

```bash
BUS=0                         # 要删除的总线号
SETUPC_DIR="<path-to-com0com-dir>"

# 阶段 1: 直接删除
pushd "$SETUPC_DIR" && ./setupc.exe remove $BUS; popd

# 阶段 2: 如果失败（端口被外部工具占用），先解绑 COM 端口名再重试
pushd "$SETUPC_DIR" && ./setupc.exe change CNCA${BUS} PortName=-; popd
pushd "$SETUPC_DIR" && ./setupc.exe change CNCB${BUS} PortName=-; popd
sleep 0.3  # 等待系统传播端口名变更
pushd "$SETUPC_DIR" && ./setupc.exe remove $BUS; popd
```

> **`PortName=-`** 将端口名恢复为内部标识（CNCAx/CNCBx），使外部工具的 COM 号引用失效，从而让 remove 成功。

### 批量清理所有端口对

```bash
SETUPC_DIR="<path-to-com0com-dir>"

# 列出所有端口对，获取 bus 号
pushd "$SETUPC_DIR" && ./setupc.exe list; popd

# 逐个删除
for bus in $(获取的 bus 号列表); do
  pushd "$SETUPC_DIR" && ./setupc.exe remove $bus; popd
done

# 或者使用 disable all → enable all 快速重置
pushd "$SETUPC_DIR" && ./setupc.exe disable all; popd
pushd "$SETUPC_DIR" && ./setupc.exe enable all; popd
```

### 完全卸载驱动

```bash
SETUPC_DIR="<path-to-com0com-dir>"

# 移除所有端口对 + 卸载内核驱动
pushd "$SETUPC_DIR" && ./setupc.exe uninstall; popd
```

> **权限要求**：`uninstall` 需要管理员权限。若当前终端无管理员权限，通过 PowerShell 提权：
> ```powershell
> powershell -NoProfile -Command "Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile','-Command','cd ''<path-to-com0com-dir>''; & ''.\setupc.exe'' uninstall'"
> ```
> **验证卸载**：`sc query com0com` 应返回 `1060: 指定的服务未安装`。

---

## 工作流 E：故障排查

### E1: os error 740（权限不足）

**症状**：setupc 输出中包含 `740`、`elevation`、`提升` 等关键字，操作失败。

**诊断**：
```bash
# 检查当前进程是否有管理员权限
net session 2>&1
# 成功 → 有管理员权限
# "Access is denied" → 没有管理员权限
```

**修复**：
1. 右键"以管理员身份运行"终端
2. 通过 PowerShell 触发 UAC 提权执行 setupc 命令（参见工作流 B — UAC 提权）
3. 如果应用程序通过清单嵌入 `requireAdministrator`，确保构建时正确嵌入了清单

### E2: 驱动文件缺失

**症状**：部分或全部 7 个必需文件缺失。

**诊断**：
```bash
SETUPC_DIR="<path-to-com0com-dir>"
for f in setupc.exe setup.dll com0com.sys com0com.inf com0com.cat cncport.inf comport.inf; do
  [ -f "$SETUPC_DIR/$f" ] && echo "✓ $f" || echo "✗ $f — 缺失！"
done
```

**修复**：
- 从 [com0com SourceForge 页面](https://sourceforge.net/projects/com0com/) 下载官方安装包
- 用 7-Zip 提取文件，确保获取正确架构（x64/x86）的文件
- 将 7 个文件放入同一目录

### E3: 幽灵端口（Ghost Ports）

**症状**：`setupc install` 报 "PortName in use" 或 "already logged"，但系统中看不到对应的 COM 端口。

**原因**：Windows COM 端口数据库中留有已卸载设备的注册表残留（幽灵设备），API 枚举检测不到，但 `setupc` 在尝试分配端口名时被 Windows 拒绝。

**诊断**：
```bash
# 查看 setupc 视角下的 COM 端口占用
pushd "$SETUPC_DIR" && ./setupc.exe busynames COM*; popd

# 对比系统层面的 COM 端口枚举（通过设备管理器或 API）
```

**修复**：
1. **跳过冲突端口**：创建端口对时准备多组候选端口号，遇到占用自动跳过
2. **手动清理幽灵设备**：
   ```cmd
   :: 以管理员身份运行 cmd.exe
   set devmgr_show_nonpresent_devices=1
   start devmgmt.msc
   :: 在设备管理器中: 查看 → 显示隐藏的设备 → 端口 (COM 和 LPT) → 右键灰色设备 → 卸载
   ```
3. **注册表清理**：清理 `HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Enum\Root\PORTS` 下的残留项（需管理员权限，操作需谨慎）

### E4: setupc.exe 执行超时

**症状**：setupc.exe 进程长时间无响应。

**原因**：驱动挂起等待、内核操作阻塞。

**诊断**：
```bash
# 检查是否有残留的 setupc.exe 进程
tasklist | findstr setupc

# 检查 Windows 事件查看器中的 com0com 驱动事件
```

**修复**：
1. 终止超时进程：`taskkill /F /IM setupc.exe`
2. 如果有多个残留 setupc.exe 进程，全部终止后重试
3. 如果问题持续：重启系统清除内核驱动挂起状态

### E5: 异常退出后的孤儿端口对

**症状**：应用程序异常退出（崩溃、强制结束进程）后，端口对未被正常清理。

**修复**：
1. **手动清理**：
   ```bash
   pushd "$SETUPC_DIR" && ./setupc.exe list; popd
   # 对每个残留 bus 号执行删除
   pushd "$SETUPC_DIR" && ./setupc.exe remove <bus>; popd
   # 或使用工作流 D 的两阶段清理策略
   ```
2. **预防**：应用程序应在启动时检测并清理残留端口对

### E8: destroy/remove exit code 1（端口对已不存在）

**症状**：`setupc remove` 返回 exit code 1，但 `setupc list` 显示端口对已经不存在。

**原因**：`setupc remove` 在端口对已不存在时返回 exit code 1（非致命"失败"），而非 exit 0。这不是错误，只是 setupc 的退出码语义。

**处理方式**：
- `setupc remove` 返回 exit code 1 时，先通过 `setupc list` 确认端口对是否真的还存在
- 如端口对已不存在，视为清理成功
- 只有确认端口对仍然存在但 remove 失败时，才进入两阶段清理流程

**手动验证**：
```bash
# 删除一个已知的端口对后再次删除，观察 exit code
pushd "$SETUPC_DIR" && ./setupc.exe remove 0; popd
echo "Exit code: $?"    # 成功时输出 0
pushd "$SETUPC_DIR" && ./setupc.exe remove 0; popd
echo "Exit code: $?"    # 已不存在时输出 1
```

---

## 工作流 F：交互式验证虚拟串口桥接

### F1: 确认端口对已创建

1. 检查端口对列表：
   ```bash
   pushd "$SETUPC_DIR" && ./setupc.exe list; popd
   # 预期输出示例:
   # CNCA0 PortName=COM22
   # CNCB0 PortName=COM23,PlugInMode=yes
   ```

### F2: 用外部工具验证数据读取

1. **打开端口 B**：用任意串口工具（PuTTY、SSCOM、Python `pyserial`）连接到端口 B（如 COM23）
   - 波特率设置与主应用程序一致
2. **从主应用程序发送数据**
3. **验证**：外部工具应能实时接收相同数据

```python
# Python 快速验证脚本（无需安装额外依赖）
import serial
ser = serial.Serial('COM23', 115200, timeout=1)
while True:
    data = ser.read(1024)
    if data:
        print(f"收到: {data}")
```

### F3: 用外部工具验证数据写入（双向桥接）

1. 外部工具保持连接端口 B
2. 在外部工具中发送数据
3. **验证**：主应用程序应显示外部工具发送的数据

### F4: 断开验证（PlugInMode 自动隐藏）

1. 在主应用程序中断开端口 A
2. **验证**：外部工具中端口 B 应立即断开或消失（`PlugInMode=yes` 保证 B 端在 A 端关闭时自动隐藏）
3. 用 `setupc list` 确认端口对已从驱动中移除

### F5: 多端口对并发验证

1. 创建多对端口（如 COM22↔COM23, COM24↔COM25）
2. 用两个外部工具分别打开 COM23 和 COM25
3. **验证**：两个外部工具应同时收到主应用程序的广播数据；各自写入的数据应分别到达主应用程序

---

## setupc 命令速查

> 完整 CLI 参考见 `references/setupc-cli.md`

### 核心命令

| 命令 | 用途 |
|------|------|
| `install 0 - -` | 安装驱动（创建临时端口对后立即删除） |
| `install <n> PortName=COMxx "PortName=COMxx,PlugInMode=yes"` | 创建端口对 |
| `remove <n>` | 删除端口对 |
| `change CNCA<n> PortName=-` | 解绑端口名 |
| `list` | 列出所有端口对 |
| `uninstall` | 完全卸载驱动和端口对 |
| `busynames COM*` | 查询已占用的 COM 端口名 |

### 端口参数语法

```
<par>=<val>[,...]    — 设置指定参数的值
-                    — 使用驱动默认值
*                    — 使用当前设置
```

### 常用端口参数

| 参数 | 取值 | 说明 |
|------|------|------|
| `PortName=<portname>` | 如 `COM22` | 设置端口名 |
| `EmuBR={yes\|no}` | yes/no | 波特率模拟 |
| `EmuOverrun={yes\|no}` | yes/no | 缓冲区超限 |
| `EmuNoise=<n>` | 0-0.99999999 | 每帧错误概率 |
| `PlugInMode={yes\|no}` | yes/no | 插件模式（对端未开时隐藏） |
| `ExclusiveMode={yes\|no}` | yes/no | 独占模式（打开时隐藏） |
| `HiddenMode={yes\|no}` | yes/no | 隐藏模式（枚举器不可见） |

### F6: 延迟测试（往返时间测量）

1. 用 Python 脚本连接到端口 B 进行往返延迟测试：
```python
import serial, time

ser = serial.Serial('COM23', 115200, timeout=0.1)

# 发送测试数据并等待回显
test_data = b'PING'
start = time.perf_counter()
ser.write(test_data)
response = ser.read(len(test_data))
elapsed = (time.perf_counter() - start) * 1000

if response == test_data:
    print(f"往返延迟: {elapsed:.1f} ms")
else:
    print(f"接收数据不匹配: 期望 {test_data}, 收到 {response}")

ser.close()
```
2. **预期结果**：单对端口延迟 < 5ms，4 对并发 < 20ms
3. 如果延迟异常高（> 100ms），参见工作流 G 诊断

---

## 工作流 G：性能排查

### G1: 多端口延迟排查

**症状**：开启多对虚拟串口（2-4 对）时，外部串口工具感觉延迟明显（"卡"）。

**诊断步骤**：

1. **确认桥接线程状态**：
   - 检查 `TauTerm_YYYYMMDD.log` 中是否有 "桥接写通道已满" 的 trace 日志
   - 大量 "已满" 消息表示物理端口写入跟不上虚拟端口读取速度

2. **测量单对 vs 多对延迟**：
   ```bash
   # 使用工作流 F6 的 Python 脚本分别测试 1 对和 4 对场景
   # 对比延迟差异
   ```

3. **检查物理端口波特率**：
   - 虚拟端口桥接不受物理波特率限制（内核驱动直接转发数据）
   - 但如果物理端口 I/O 线程在低波特率下处理大块数据，可能阻塞 SessionStore Mutex

4. **检查 CPU 占用**：
   - 桥接线程使用 `recv_timeout(10ms)` + `read(timeout=5ms)` 策略
   - 空闲时 CPU 占用应接近 0%

**常见原因与修复**：

| 原因 | 表现 | TauTerm 优化 |
|------|------|-------------|
| 虚拟端口读取超时过长 | 4 对端口每轮循环等待 200ms | v0.3.1+ 超时已优化为 5ms/端口 |
| 写回物理端口时 Mutex 争用 | 桥接线程等待 I/O 线程释放锁 | v0.3.1+ 使用独立写线程 + channel 解耦 |
| 外部工具缓冲区满 | 外部工具处理能力不足 | 降低物理端口数据速率，或增加外部工具缓冲区 |
| com0com 驱动内核层积压 | `setupc.exe list` 正常但数据延迟 | 重启 TauTerm 重建端口对 |

### G2: 桥接线程诊断

**查看 TauTerm 日志**：
```bash
# 搜索桥接相关日志
findstr /i "桥接" TauTerm_*.log
findstr /i "bridge" TauTerm_*.log
```

关键日志消息：
- `虚拟端口 COMxx 已打开（桥接）` — 正常启动
- `桥接写通道已满，丢弃 N 字节` — 写入线程处理跟不上（可能需要增大 channel 容量）
- `桥接写线程退出` — 会话断开，写线程正常退出
- `桥接线程退出` — 桥接主循环正常退出
- `桥接线程 panic` — 异常退出，检查前后日志确定原因

### G3: Channel 容量调优

如果日志中出现大量 "桥接写通道已满" 消息，可调整 channel 容量：

- **桥接数据 channel**（物理 → 虚拟，容量 256）: 在 `commands.rs` 中搜索 `sync_channel::<Vec<u8>>(256)`
- **写回 channel**（虚拟 → 物理，容量 128）: 在 `commands.rs` 中搜索 `sync_channel::<Vec<u8>>(128)`
- **I/O 循环写 channel**（容量 32）: 在 `session_store.rs` 中搜索 `sync_channel::<IoLoopCmd>(32)`

增大容量可以缓冲突发数据，但也会增加内存占用。

---

## 平台说明

com0com 仅支持 **Windows**。Linux 平台可通过 **socat** (PTY 对) 或 **tty0tty** 内核模块实现类似功能，macOS 可使用内置的 `socat` 或 IOKit 虚拟串口。
