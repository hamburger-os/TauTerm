# com0com Virtual Serial Port Driver

## 文件来源

com0com 驱动文件提取自官方已签名的安装包（SourceForge）：

- `Setup_com0com_v3.0.0.0_W7_x64_signed.exe` → 提取到 `x64/`（7 个文件）
- `Setup_com0com_v3.0.0.0_W7_x86_signed.exe` → 提取到 `x86/`（7 个文件）

提取工具：7-Zip（NSIS 格式安装包）
提取命令：`7z e <installer.exe> -o<output_dir>`

## 完整文件清单（每个架构 7 个文件）

```
resources/com0com/
├── x64/                   # 64-bit 驱动文件（与 x86/ 的区别：sys/dll/exe/cat 为 64 位版本）
│   ├── setupc.exe         # 命令行管理工具（必需）
│   ├── setup.dll          # setupc.exe 运行时依赖 DLL（必需）
│   ├── com0com.sys        # 内核驱动（必需）
│   ├── com0com.inf        # 驱动安装信息 INF（必需）
│   ├── com0com.cat        # 安全目录 — 驱动签名验证（必需）
│   ├── cncport.inf        # 端口配置 INF — setupc.exe 安装时引用（必需）
│   ├── comport.inf        # 端口行为配置 INF（必需）
│   ├── setupg.exe         # GUI 管理工具（可选，未打包）
│   └── ReadMe.txt         # com0com 上游说明（可选，未打包）
└── x86/                   # 32-bit 驱动文件（结构与 x64/ 完全一致）
    ├── setupc.exe
    ├── setup.dll
    ├── com0com.sys
    ├── com0com.inf
    ├── com0com.cat
    ├── cncport.inf
    ├── comport.inf
    ├── setupg.exe
    └── ReadMe.txt
```

> **打包规则**：`build.rs` 只复制标记为"必需"的 7 个文件到打包根目录。`setupg.exe` 和 `ReadMe.txt` 用于调试/参考，不随安装包分发。

## 为什么需要 7 个文件

| 文件 | 定位 | 缺少时的后果 |
|------|------|-------------|
| `setupc.exe` | 调用入口 | 无法执行任何 com0com 操作 |
| `setup.dll` | `setupc.exe` 的运行时 DLL | `setupc.exe` 启动报错：找不到 SETUP.dll |
| `com0com.sys` | Windows 内核驱动 | 驱动安装失败 |
| `com0com.inf` | 驱动安装信息 | 驱动安装失败 |
| `com0com.cat` | 驱动签名安全目录 | 驱动签名验证失败（需禁用签名强制） |
| `cncport.inf` | COM 端口配置 | `SetupOpenInfFile(cncport.inf)` 错误 |
| `comport.inf` | COM 端口行为配置 | 端口行为异常或安装失败 |

> 这些文件是一个整体，缺一不可。提取时必须保留安装包内的全部驱动相关文件。

## 构建流程

1. `build.rs` 根据目标架构（`CARGO_CFG_TARGET_ARCH`）将 7 个必需文件复制到 `resources/com0com/` 根目录
2. `tauri.conf.json` 的 `bundle.resources` 将根目录下的文件打包到安装程序（glob: `../resources/com0com/*`）
3. NSIS 安装程序配置为 `installMode: "perMachine"`，在安装时强制请求 UAC 管理员提权
4. NSIS post-install hook 在提权环境下执行 `setupc.exe install` 完成驱动安装
5. 卸载时 pre-uninstall hook 执行 `setupc.exe uninstall` 移除驱动

## 运行时回退

如果 NSIS 安装时驱动安装失败（如用户拒绝了 UAC 提权），TauTerm 提供多层恢复路径：

1. **首次连接串口**：后端自动尝试运行时安装（当前进程已提权时成功）
2. **状态栏一键修复**：VPort 失败时状态栏显示"修复"按钮，点击后：
   - 先尝试直接安装（已提权进程）
   - 若失败则在 Windows 上通过 PowerShell `Start-Process -Verb RunAs` 触发 UAC 提权安装
3. **启动时主动提醒**：应用启动时若检测到驱动未安装，向前端发送事件，状态栏持续显示警告和修复入口

## 管理员权限说明

com0com 是 Windows 内核驱动，驱动安装、端口对创建/销毁均需要管理员权限。

TauTerm 采用**双层提权**策略确保虚拟串口功能始终可用：

### 1. 启动即提权（Manifest）
- `build.rs` 在 Windows 可执行文件中嵌入 `requireAdministrator` 清单（`src-tauri/windows/manifest.xml`）
- 用户每次启动 TauTerm 时 Windows 自动弹出 UAC 提权提示
- 因为运行时创建/销毁虚拟端口对也需要管理员权限，这种方式比每次操作都弹 UAC 体验更好
- 用户拒绝 UAC 提权 → 应用无法启动（与内核驱动操作需求一致）

### 2. 安装时提权（NSIS）
- `tauri.conf.json` 配置 `installMode: "perMachine"` → NSIS 安装程序在安装时请求提权
- NSIS post-install hook 在提权环境下执行 `setupc.exe install` 完成驱动安装
- 卸载时 NSIS pre-uninstall hook 执行 `setupc.exe uninstall` 移除驱动

### 3. 运行时回退（故障恢复）
若驱动被意外卸载或损坏，TauTerm 提供多层恢复路径：
1. **首次连接串口**：后端自动尝试运行时安装（当前进程已提权时成功）
2. **状态栏一键修复**：VPort 失败时状态栏显示"修复"按钮，在当前已提权进程中直接安装
3. **启动时主动提醒**：应用启动时若检测到驱动未安装，向前端发送事件，状态栏持续显示警告和修复入口

## setupc 命令行说明

```
setupc help:
Setup for com0com

Usage:
  C:\Program Files\TauTerm\setupc.exe [options] <command>

Options:
  --output <file>              - file for output, default is console
  --wait [+]<to>               - wait <to> seconds for install completion. If
                                 <to> has '+' prefix then ask user to continue
                                 waiting after <to> seconds elapsing
                                 (by default <to> is 0 - no wait)
  --detail-prms                - show detailed parameters
  --silent                     - suppress dialogs if possible
  --no-update                  - do not update driver while install command
                                 execution (the other install command w/o this
                                 option expected later)
  --no-update-fnames           - do not update friendly names
  --show-fnames                - show friendly names activity

Commands:
  install <n> <prmsA> <prmsB>  - install a pair of linked ports with
   or                            identifiers CNCA<n> and CNCB<n>
  install <prmsA> <prmsB>        (by default <n> is the first not used number),
                                 set their parameters to <prmsA> and <prmsB>
  install                      - can be used to update driver after execution
                                 of install commands with --no-update option
  remove <n>                   - remove a pair of linked ports with
                                 identifiers CNCA<n> and CNCB<n>
  disable all                  - disable all ports in current hardware profile
  enable all                   - enable all ports in current hardware profile
  change <portid> <prms>       - set parameters <prms> for port with
                                 identifier <portid>
  list                         - for each port show its identifier and
                                 parameters
  preinstall                   - preinstall driver
  update                       - update driver
  reload                       - reload driver
  uninstall                    - uninstall all ports and the driver
  infclean                     - clean old INF files
  busynames <pattern>          - show names that already in use and match the
                                 <pattern> (wildcards: '*' and '?')
  updatefnames                 - update friendly names
  listfnames                   - for each bus and port show its identifier and
                                 friendly name
  quit                         - quit
  help                         - print this help

Syntax of port parameters string:
  -                       - use driver's defaults for all parameters
  *                       - use current settings for all parameters
  <par>=<val>[,...]       - set value <val> for each parameter <par>

Parameters:
  PortName=<portname>     - set port name to <portname>
                            (port identifier by default)
  EmuBR={yes|no}          - enable/disable baud rate emulation in the direction
                            to the paired port (disabled by default)
  EmuOverrun={yes|no}     - enable/disable buffer overrun (disabled by default)
  EmuNoise=<n>            - probability in range 0-0.99999999 of error per
                            character frame in the direction to the paired port
                            (0 by default)
  AddRTTO=<n>             - add <n> milliseconds to the total time-out period
                            for read operations (0 by default)
  AddRITO=<n>             - add <n> milliseconds to the maximum time allowed to
                            elapse between the arrival of two characters for
                            read operations (0 by default)
  PlugInMode={yes|no}     - enable/disable plug-in mode, the plug-in mode port
                            is hidden and can't be open if the paired port is
                            not open (disabled by default)
  ExclusiveMode={yes|no}  - enable/disable exclusive mode, the exclusive mode
                            port is hidden if it is open (disabled by default)
  HiddenMode={yes|no}     - enable/disable hidden mode, the hidden mode port is
                            hidden as it is possible for port enumerators
                            (disabled by default)
  AllDataBits={yes|no}    - enable/disable all data bits transfer disregard
                            data bits setting (disabled by default)
  cts=[!]<p>              - wire CTS pin to <p> (rrts by default)
  dsr=[!]<p>              - wire DSR pin to <p> (rdtr by default)
  dcd=[!]<p>              - wire DCD pin to <p> (rdtr by default)
  ri=[!]<p>               - wire RI pin to <p> (!on by default)

The possible values of <p> above can be rrts, lrts, rdtr, ldtr, rout1, lout1,
rout2, lout2 (remote/local RTS/DTR/OUT1/OUT2), ropen, lopen (logical ON if
remote/local port is open) or on (logical ON). The exclamation sign (!) can be
used to invert the value.

Special values:
  -                       - use driver's default value
  *                       - use current setting

If parameter 'PortName=COM#' is used then the Ports class installer will be
invoked to set the real port name. The Ports class installer selects the COM
port number and sets the real port name to COM<n>, where <n> is the selected
port number. Thereafter use parameter RealPortName=COM<n> to change the real
port name.

Examples:
  C:\Program Files\TauTerm\setupc.exe install - -
  C:\Program Files\TauTerm\setupc.exe install 5 * *
  C:\Program Files\TauTerm\setupc.exe remove 0
  C:\Program Files\TauTerm\setupc.exe install PortName=COM2 PortName=COM4
  C:\Program Files\TauTerm\setupc.exe install PortName=COM5,EmuBR=yes,EmuOverrun=yes -
  C:\Program Files\TauTerm\setupc.exe change CNCA0 EmuBR=yes,EmuOverrun=yes
  C:\Program Files\TauTerm\setupc.exe change CNCA0 PortName=-
  C:\Program Files\TauTerm\setupc.exe list
  C:\Program Files\TauTerm\setupc.exe uninstall
  C:\Program Files\TauTerm\setupc.exe busynames COM?*
```
