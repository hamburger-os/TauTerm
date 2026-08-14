# setupc.exe 完整命令行参考

> 来源：com0com v3.0.0.0，文件路径 `resources/com0com/x64/setupc.exe`

## 概述

`setupc.exe` 是 com0com 虚拟串口驱动的命令行管理工具。所有操作需要**管理员权限**。

## 用法

```
setupc [options] <command>
```

## 全局选项

| 选项 | 说明 |
|------|------|
| `--output <file>` | 输出到文件，默认为控制台 |
| `--wait [+] <to>` | 等待 `<to>` 秒完成安装。若前缀 `+` 则在超时后询问是否继续等待（默认 0 — 不等待） |
| `--detail-prms` | 显示详细参数 |
| `--silent` | 尽可能抑制对话框 |
| `--no-update` | 执行 install 时不立即更新驱动（预期后续会执行无此选项的 install） |
| `--no-update-fnames` | 不更新友好名称 |
| `--show-fnames` | 显示友好名称活动 |

## 命令

### install — 安装端口对 / 更新驱动

```
install <n> <prmsA> <prmsB>  使用标识符 CNCA<n> 和 CNCB<n> 安装一对链接端口，
                              设置参数为 <prmsA> 和 <prmsB>（默认 <n> 为第一个未使用的编号）

install <prmsA> <prmsB>      安装链接端口对（自动分配总线号）

install                       更新驱动（用于 --no-update 模式后）
```

**示例**：
```bash
# 使用全部默认参数，自动分配总线号
setupc.exe install - -

# 在总线 5 上使用当前设置创建端口对（复制已有对）
setupc.exe install 5 * *

# 指定 COM 端口号
setupc.exe install PortName=COM2 PortName=COM4

# 指定端口名 + 参数
setupc.exe install PortName=COM5,EmuBR=yes,EmuOverrun=yes -

# 典型用法：A 侧指定 COM 号，B 侧加 PlugInMode（B 端口在 A 未开时自动隐藏）
setupc.exe install 0 PortName=COM22 "PortName=COM23,PlugInMode=yes"
```

### remove — 删除端口对

```
remove <n>   删除标识符为 CNCA<n> 和 CNCB<n> 的链接端口对
```

**示例**：
```bash
setupc.exe remove 0
setupc.exe --silent remove 0    # 静默模式（抑制对话框）
```

### change — 修改端口参数

```
change <portid> <prms>   为标识符为 <portid> 的端口设置参数 <prms>
```

**示例**：
```bash
# 修改 CNCA0 的参数
setupc.exe change CNCA0 EmuBR=yes,EmuOverrun=yes

# 将端口名恢复为内部标识（解绑 COM 号 — 两阶段清理策略阶段 2）
setupc.exe change CNCA0 PortName=-

# 更改实际 COM 端口号
setupc.exe change CNCA0 RealPortName=COM5
```

### list / listfnames — 列出端口

```
list           显示每个端口的标识符和参数
listfnames     显示每个总线和端口的标识符和友好名称
```

**示例**：
```bash
setupc.exe list
# 输出示例:
# CNCA0 PortName=COM22
# CNCB0 PortName=COM23,PlugInMode=yes

setupc.exe listfnames
```

### busynames — 查询已占用的 COM 端口名

```
busynames <pattern>   显示已占用且匹配 <pattern> 的名称（通配符: '*' 和 '?'）
```

**示例**：
```bash
setupc.exe busynames COM?*    # 列出所有已占用的 COM 端口名
setupc.exe busynames COM1*    # 列出已占用的 COM1x 端口
```

### disable all / enable all — 禁用/启用所有端口

```
disable all   在当前硬件配置文件中禁用所有端口
enable all    在当前硬件配置文件中启用所有端口
```

### 驱动级操作

```
preinstall    预安装驱动
update        更新驱动
reload        重新加载驱动
uninstall     卸载所有端口和驱动
infclean      清理旧的 INF 文件
updatefnames  更新友好名称
```

**示例**：
```bash
# 完全清理（卸载时使用）
setupc.exe uninstall

# 重新加载驱动（不需要重启）
setupc.exe reload
```

### quit / help

```
quit          退出
help          打印帮助信息
```

## 端口参数语法

```
<par>=<val>[,...]    设置各参数的值
-                    使用驱动的默认值（全部参数）
*                    使用当前设置（全部参数）
```

`-` 和 `*` 可分别在 A/B 端口侧独立使用：
```bash
# A 侧使用默认值，B 侧使用当前设置
setupc.exe install - *
```

## 端口参数一览

### 基本参数

| 参数 | 取值 | 默认值 | 说明 |
|------|------|--------|------|
| `PortName=<portname>` | 如 `COM22` | 端口标识符 | 设置端口名称。若使用 `PortName=COM#`，Ports 类安装程序会选择 COM 端口号并将实际端口名设置为 `COM<n>`。之后可使用 `RealPortName=COM<n>` 更改 |
| `RealPortName=COM<n>` | 如 `COM5` | — | 直接更改实际 COM 端口号（绕过 Ports 类安装程序的选择逻辑） |

### 模拟参数

| 参数 | 取值 | 默认值 | 说明 |
|------|------|--------|------|
| `EmuBR={yes\|no}` | yes / no | no | 启用/禁用波特率模拟（向配对端口方向） |
| `EmuOverrun={yes\|no}` | yes / no | no | 启用/禁用缓冲区超限 |
| `EmuNoise=<n>` | 0–0.99999999 | 0 | 向配对端口方向每字符帧的错误概率 |

### 超时参数

| 参数 | 取值 | 默认值 | 说明 |
|------|------|--------|------|
| `AddRTTO=<n>` | 毫秒数 | 0 | 为读操作的总超时时间添加 `<n>` 毫秒 |
| `AddRITO=<n>` | 毫秒数 | 0 | 为读操作的两字符间隔最大时间添加 `<n>` 毫秒 |

### 模式参数

| 参数 | 取值 | 默认值 | 说明 |
|------|------|--------|------|
| `PlugInMode={yes\|no}` | yes / no | no | 插件模式 — 端口隐藏，对端未打开时无法打开 |
| `ExclusiveMode={yes\|no}` | yes / no | no | 独占模式 — 端口打开时隐藏 |
| `HiddenMode={yes\|no}` | yes / no | no | 隐藏模式 — 端口对枚举器尽可能隐藏 |
| `AllDataBits={yes\|no}` | yes / no | no | 忽略数据位设置，传输所有数据位 |

### 引脚连接参数

```
cts=[!]<p>    连接 CTS 引脚到 <p>（默认 rrts）
dsr=[!]<p>    连接 DSR 引脚到 <p>（默认 rdtr）
dcd=[!]<p>    连接 DCD 引脚到 <p>（默认 rdtr）
ri=[!]<p>     连接 RI 引脚到 <p>（默认 !on）
```

**`<p>` 的可能值**：

| 值 | 含义 |
|----|------|
| `rrts` | 远程 RTS |
| `lrts` | 本地 RTS |
| `rdtr` | 远程 DTR |
| `ldtr` | 本地 DTR |
| `rout1` | 远程 OUT1 |
| `lout1` | 本地 OUT1 |
| `rout2` | 远程 OUT2 |
| `lout2` | 本地 OUT2 |
| `ropen` | 远程端口打开时为逻辑 ON |
| `lopen` | 本地端口打开时为逻辑 ON |
| `on` | 逻辑 ON |

`!` 前缀表示取反。

## 参数特殊值

| 值 | 含义 |
|----|------|
| `-` | 使用驱动的默认值 |
| `*` | 使用当前设置 |

## 常见退出码

| 退出码 | 含义 |
|--------|------|
| 0 | 成功 |
| 1 | 常规错误（含端口对已不存在、端口名冲突等） |
| 其他 | 特定错误（如权限不足、文件缺失等） |
