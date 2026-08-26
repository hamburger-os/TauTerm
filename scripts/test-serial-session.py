#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
TauTerm 串口会话集成测试假服务器 — 模拟 RT-Thread 设备

本脚本在 Windows 上通过 com0com 虚拟串口驱动创建一个端口对（默认 COM200 ↔ COM201），
其一端（近端 COM200）由本脚本用 pyserial 打开，作为"RT-Thread 设备"；
另一端（远端 COM201）由 TauTerm 的串口会话连接，作为"上位机终端"。

    脚本 (pyserial) ──> 近端 COM200 ══ com0com 内核桥 ══ 远端 COM201 ──> TauTerm

注意（预留区约定）：本脚本固定使用预留端口段 COM200-COM255 与预留 bus 段 200-255，
与产品/TauTermService 使用的端口/bus 天然隔离，可与之同时运行而不互删、不互占。
这两段常量与 src-tauri/src/virtual_port/manager.rs 的 RESERVED_* 必须一致
（由 scripts/check-reserved-region.js 校验）。

连接后脚本会向 TauTerm 输出 RT-Thread 启动横幅并进入 FinSH(msh) 交互 Shell，
模拟真实 RT-Thread 设备的串口输入输出，用于手动验证 TauTerm 串口会话功能。

覆盖的测试点：
  1. 基础终端交互 —— 回显 / 退格 / 回车 / Ctrl-C / 命令解析 / 历史命令
  2. RT-Thread FinSH 标准命令集 —— help / version / ps / free / ls / cd / pwd / date 等
  3. 字符集 / HEX 视图 —— charset 命令输出 UTF-8 与 GBK 样例字节，配合 TauTerm 的字符集/HEX 视图观察差异
  4. X / Y / ZModem 文件传输 —— sx/sy/sz(发送给上位机) 与 rx/ry/rz(从上位机接收)
  5. 自动应答 / Lua 脚本 —— --respond 加载规则文件，对收到的行做模式匹配并应答

依赖（按需，缺失时对应功能会给出安装提示，Shell 仍可用）：
  · pyserial        —— 必须（通常 Anaconda 已自带）
  · xmodem          —— 可选，XModem 传输，pip install xmodem
  · ymodem          —— 可选，YModem 传输，pip install ymodem
  · zmodem 库在 PyPI 无稳定包 —— ZModem 传输使用内置标准实现（单文件 rz/sz，独立于 TauTerm）

用法示例：
  # 只清理预留段内残留的 com0com 端口对（需管理员；不影响产品端口对）
  python scripts/test-serial-session.py --teardown-all

  # 自动创建端口对 COM200/COM201（需管理员）并以高仿真模式启动
  python scripts/test-serial-session.py --setup --near COM200 --far COM201

  # 端口对已手动创建好，直接连接近端启动（无需管理员）
  python scripts/test-serial-session.py --near COM200

  # 打开 HEX 调试视图，记录所有收/发字节的十六进制
  python scripts/test-serial-session.py --near COM200 --hex

  # 加载自动应答规则文件（模拟 Lua/脚本驱动测试）
  python scripts/test-serial-session.py --near COM200 --respond rules.txt

  # 用 Anaconda 自带的 python 启动
  C:\\ProgramData\\anaconda3\\python.exe scripts/test-serial-session.py --near COM200

在 TauTerm 中新建"串口"会话，选择远端端口（如 COM201），波特率与本脚本默认一致（115200），
即可看到 RT-Thread 启动横幅与 `msh />` 提示符。

注意：
  · com0com 端口对的创建/删除需要管理员权限；仅打开已存在的端口不需要。
  · 本脚本固定使用预留端口段 COM200-COM255 与预留 bus 段 200-255，产品/TauTermService
    会主动避开该段，两者可同时运行而不互删、不互占。
  · 本脚本退出时会自动清理它在预留段创建的端口对（若以 --setup 创建）。
  · 若上次清理失败（如对端仍被 TauTerm 占用），可能残留"僵尸"端口对或导致内核驱动
    停止；--setup 会在创建前自动修复（启动已停止的驱动、清理未绑定端口名的残留对）。
  · Windows 上本脚本依赖 com0com 虚拟串口驱动；非 Windows 平台请改用 socat / tty0tty 等其他方案。
"""
import argparse
import binascii
import os
import re
import shlex
import struct
import subprocess
import sys
import threading
import time
from collections import deque

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(line_buffering=True)

try:
    import serial  # pyserial
except ImportError:  # pragma: no cover
    print("[错误] 缺少 pyserial，请先安装：pip install pyserial")
    sys.exit(1)

# ─────────────────────────────────────────────────────────────────────────────
# 路径与常量
# ─────────────────────────────────────────────────────────────────────────────

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(SCRIPT_DIR)
# com0com 配套文件（setupc.exe 与其 6 个配套文件）所在目录
COM0COM_DIR = os.path.join(REPO_ROOT, "resources", "com0com", "x64")
SETUPC = os.path.join(COM0COM_DIR, "setupc.exe")

DEFAULT_BAUD = 115200
FINSH_VERSION = "5.2.2"

# ── 预留区（与 src-tauri/src/virtual_port/manager.rs 的 RESERVED_* 保持一致）──
# 约定：预留段仅供本测试脚本使用。产品扫描端口/bus 时须避开，且 TauTermService
# 启动的孤儿清理不得触碰预留 bus 段。两边常量由 scripts/check-reserved-region.js
# 在构建时校验一致性，修改时务必同步更新 Rust 侧。
RESERVED_PORT_BASE = 200
RESERVED_PORT_END = 255
RESERVED_BUS_BASE = 200
RESERVED_BUS_END = 255
# 测试脚本固定的近端/远端端口
NEAR_PORT = "COM200"
FAR_PORT = "COM201"


# ─────────────────────────────────────────────────────────────────────────────
# 管理员权限 / com0com 管理
# ─────────────────────────────────────────────────────────────────────────────

def is_admin():
    """当前进程是否以管理员权限运行（非 Windows 恒返回 True）。"""
    if sys.platform == "win32":
        try:
            import ctypes

            return bool(ctypes.windll.shell32.IsUserAnAdmin())
        except Exception:
            return False
    return True


def _setupc(args, timeout=10):
    """在 com0com 配套目录中执行 setupc.exe，返回 (exitcode, stdout, stderr)。

    setupc.exe 必须在其配套文件（com0com.inf 等 7 个文件）所在目录运行，否则报错。
    """
    if not os.path.isfile(SETUPC):
        return -1, "", f"找不到 {SETUPC}（com0com 配套文件不完整）"
    try:
        proc = subprocess.run(
            [SETUPC] + args,
            cwd=COM0COM_DIR,
            capture_output=True,
            text=True,
            timeout=timeout,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        return proc.returncode, proc.stdout or "", proc.stderr or ""
    except subprocess.TimeoutExpired:
        return -1, "", "setupc.exe 执行超时（可先 taskkill /F /IM setupc.exe 再重试）"
    except OSError as e:
        return -1, "", f"setupc.exe 调用失败: {e}"


def com_list_pairs():
    """解析 setupc list，返回 [{bus, port_a, port_b, whole}...]。"""
    code, out, err = _setupc(["list"])
    pairs = []
    if code != 0 and not out:
        return pairs
    current = None
    for raw in out.splitlines():
        line = raw.strip()
        m = re.match(r"(CNCA|CNCB)(\d+)\s+(.*)$", line)
        if not m:
            continue
        side, bus, rest = m.group(1), int(m.group(2)), m.group(3)
        pm = re.search(r"PortName=([^,\s]+)", rest)
        port = pm.group(1) if pm else f"CNCA{bus}" if side == "CNCA" else f"CNCB{bus}"
        if current is None or current["bus"] != bus:
            current = {"bus": bus, "port_a": None, "port_b": None, "raw": []}
            pairs.append(current)
        if side == "CNCA":
            current["port_a"] = port
        else:
            current["port_b"] = port
        current["raw"].append(line)
    return pairs


def com_find_bus_for_port(port):
    """根据端口名查找其所在 bus 号；未找到返回 None。"""
    for p in com_list_pairs():
        if port and (p["port_a"] == port.upper() or p["port_b"] == port.upper()):
            return p["bus"]
    return None


def com_busy_ports():
    code, out, err = _setupc(["busynames", "COM*"])
    if code != 0:
        return set()
    return {tok for tok in re.split(r"[\s,]+", out) if re.fullmatch(r"COM\d+", tok.strip())}


def _ensure_admin_hint(action):
    print(f"[错误] 需要管理员权限才能{action} com0com 端口对。")
    print("  请以管理员身份运行本脚本，或使用下面的 PowerShell 提权命令：")
    cmd = ("Start-Process powershell -Verb RunAs -Wait -WindowStyle Hidden "
           "-ArgumentList '-NoProfile','-Command',"
           f"'cd \\\"{COM0COM_DIR}\\\"; & \\\".\\\\setupc.exe\\\" {action}'")
    print(f"  PS> {cmd}")
    print("  提示：os error 740 = 权限不足（ELEVATION_REQUIRED）。")


def com_create_pair(near, far):
    """创建端口对，返回 (bus, 错误信息或 None)。

    若指定端口号被占用，会尝试跳过。setupc remove 在端口对已不存在时返回 exit code 1，
    属正常语义，不作为错误。
    """
    if not is_admin():
        _ensure_admin_hint("install")
        return None, "需要管理员权限"

    busy = com_busy_ports()
    used_near = near.upper() in busy or com_find_bus_for_port(near) is not None
    used_far = far.upper() in busy or com_find_bus_for_port(far) is not None
    if used_near or used_far:
        return None, f"端口被占用: {near}({'占用' if used_near else '空闲'}) {far}({'占用' if used_far else '空闲'})"

    # 测试脚本固定使用预留 bus 段，且只占用该段内的 bus —— 与产品（其 bus 始终
    # 低于预留段）天然隔离，杜绝与产品并发抢占同一 bus。若指定端口已被占用，
    # 前面的 used_near/used_far 已提前返回错误。
    used_buses = {p["bus"] for p in com_list_pairs()}
    bus = next((b for b in range(RESERVED_BUS_BASE, RESERVED_BUS_END + 1)
                if b not in used_buses), None)
    if bus is None:
        return None, f"预留 bus 段 ({RESERVED_BUS_BASE}-{RESERVED_BUS_END}) 已用尽"

    a_param = f"PortName={near}"
    b_param = f"PortName={far},PlugInMode=yes"
    code, out, err = _setupc(["install", str(bus), a_param, b_param])
    if code != 0:
        # 兜底：可能是瞬时占用冲突，稍后重试低一位
        return None, (err or out).strip() or f"exit code {code}"
    return bus, None


def com_remove_port(port):
    """删除包含指定端口名的端口对（两阶段清理）。"""
    if not is_admin():
        _ensure_admin_hint("remove")
        return False
    bus = com_find_bus_for_port(port)
    if bus is None:
        print(f"[信息] 端口 {port} 未注册为 com0com 端口对，无需删除。")
        return True
    return _remove_bus(bus)


def _remove_bus(bus):
    # 阶段一：直接删除
    code, out, err = _setupc(["remove", str(bus)])
    if code == 0:
        return True
    # 阶段二：先解绑端口名再删除（端口可能被外部工具占用）
    _setupc(["change", f"CNCA{bus}", "PortName=-"])
    _setupc(["change", f"CNCB{bus}", "PortName=-"])
    time.sleep(0.3)
    code, out, err = _setupc(["remove", str(bus)])
    if code != 0:
        detail = (err or out).strip()
        if detail:
            print(f"[警告] 删除 bus {bus} 失败: {detail}")
        else:
            print(f"[警告] 删除 bus {bus} 失败（setupc remove 返回 {code}）。")
        print("  可能原因：对端端口仍被 TauTerm 串口会话占用，com0com 无法删除正在使用的端口对。")
        print("  处理：先关闭 TauTerm 中的串口会话，再运行 "
              "--teardown-port <PORT> 或 --teardown-all 清理残留端口对。")
        return False
    return True


def com_teardown_all():
    """删除预留段内全部 com0com 端口对（产品/TauTerm 的端口对不受影响）。"""
    targets = [p["bus"] for p in com_list_pairs()
               if RESERVED_BUS_BASE <= p["bus"] <= RESERVED_BUS_END]
    if not targets:
        print(f"[信息] 预留段 ({RESERVED_BUS_BASE}-{RESERVED_BUS_END}) 内没有 com0com 端口对，无需清理。")
        return
    for bus in targets:
        _remove_bus(bus)
    print(f"[信息] 已清理预留段 com0com 端口对: {targets}。产品端口对未受影响。")


def com_driver_state():
    """查询 com0com 内核驱动运行状态。返回 'RUNNING'/'STOPPED'/'NOT_INSTALLED'/'UNKNOWN'。"""
    try:
        proc = subprocess.run(
            ["sc", "query", "com0com"],
            capture_output=True, text=True, timeout=10,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
    except Exception:
        return "UNKNOWN"
    out = (proc.stdout or "") + "\n" + (proc.stderr or "")
    if "RUNNING" in out:
        return "RUNNING"
    if "STOPPED" in out:
        return "STOPPED"
    if "1060" in out or "does not exist" in out.lower() or "未安装" in out:
        return "NOT_INSTALLED"
    return "UNKNOWN"


def com_start_driver():
    """尝试启动 com0com 内核驱动（需管理员）。返回 (成功?, 详情)。"""
    try:
        proc = subprocess.run(
            ["sc", "start", "com0com"],
            capture_output=True, text=True, timeout=20,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
    except Exception as e:
        return False, str(e)
    detail = (proc.stdout or "").strip() or (proc.stderr or "").strip()
    return proc.returncode == 0, detail


def com_repair():
    """修复 com0com 环境：启动已停止的驱动、清理僵尸（未绑定端口名）端口对。

    返回 True 表示环境就绪；False 表示仍存在问题（原因已打印）。
    """
    if not is_admin():
        _ensure_admin_hint("repair")
        return False

    state = com_driver_state()
    if state == "STOPPED":
        print("[信息] 检测到 com0com 内核驱动已停止，正在尝试启动...")
        ok, detail = com_start_driver()
        if not ok:
            print(f"[警告] 启动 com0com 驱动失败: {detail or '未知错误'}")
            print("  可尝试重启系统后重试，或手动执行 `sc start com0com`。")
            return False
        print("[信息] com0com 内核驱动已启动。")
    elif state == "NOT_INSTALLED":
        print("[错误] com0com 内核驱动未安装。")
        print("  请先安装驱动（用 setupc install 创建临时端口对可触发驱动安装）。")
        return False

    # 清理僵尸端口对：端口名未绑定为 COMx（上次清理失败留下的 CNCAx/CNCBx）
    for p in com_list_pairs():
        a = p["port_a"] or ""
        b = p["port_b"] or ""
        if not re.fullmatch(r"COM\d+", a) or not re.fullmatch(r"COM\d+", b):
            print(f"[信息] 清理僵尸端口对 (bus {p['bus']}): {a or '(空)'} <-> {b or '(空)'}")
            _remove_bus(p["bus"])
    return True


def com_dump_plan(near, far):
    """打印将要执行的 setupc 命令（--dry-run）。"""
    print(f"# 确认 com0com 配套文件目录存在: {COM0COM_DIR}")
    for f in ["setupc.exe", "setup.dll", "com0com.sys", "com0com.inf",
              "com0com.cat", "cncport.inf", "comport.inf"]:
        ok = os.path.isfile(os.path.join(COM0COM_DIR, f))
        print(f"#   [{'√' if ok else '×'}] {f}")
    a_param = f"PortName={near}"
    b_param = f"PortName={far},PlugInMode=yes"
    # --dry-run 仅示意：实际创建时在预留 bus 段内取一个空闲 bus（见 com_create_pair）
    print(f"# 创建端口对（bus 取预留段 {RESERVED_BUS_BASE}-{RESERVED_BUS_END} 内空闲值，示例用 {RESERVED_BUS_BASE}）:")
    print(f'  cd {COM0COM_DIR} && .\\setupc.exe install {RESERVED_BUS_BASE} {a_param} "{b_param}"')
    print(f"# 查询/占用检查:")
    print(f"  cd {COM0COM_DIR} && .\\setupc.exe busynames COM*")
    print(f"  cd {COM0COM_DIR} && .\\setupc.exe list")


# ─────────────────────────────────────────────────────────────────────────────
# RT-Thread FinSH 命令输出
# ─────────────────────────────────────────────────────────────────────────────

# ── 时间/版本辅助（模拟 C 的 __DATE__ / __TIME__ / ctime） ─────────────
_MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
           "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
_DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]


def _c_date():
    """模拟 C 宏 __DATE__ 的格式：`Aug 24 2026`。"""
    t = time.localtime()
    return f"{_MONTHS[t.tm_mon - 1]} {t.tm_mday:2d} {t.tm_year}"


def _c_time():
    """模拟 C 宏 __TIME__ 的格式：`HH:MM:SS`。"""
    return time.strftime("%H:%M:%S")


def _c_ctime():
    """模拟 C 标准库 ctime() 的输出：`Mon Aug 24 10:00:00 2026`。"""
    t = time.localtime()
    return (f"{_DAYS[t.tm_wday]} {_MONTHS[t.tm_mon - 1]} {t.tm_mday:2d} "
            f"{t.tm_hour:02d}:{t.tm_min:02d}:{t.tm_sec:02d} {t.tm_year}")


def _build_banner():
    """对应 rt_show_version() 的输出（前导空行 + 版本 + 构建时间 + 版权）。"""
    return (
        "\r\n \\ | /\r\n"
        "- RT -     Thread Operating System\r\n"
        f" / | \\     {FINSH_VERSION} build {_c_date()} {_c_time()}\r\n"
        " 2006 - 2024 Copyright by RT-Thread team\r\n"
    )


# ── 命令表（help 与 Tab 补全共用；原生命令描述沿用 RT-Thread 源码） ─────
COMMANDS = [
    ("help", "RT-Thread shell help"),
    ("version", "show RT-Thread version information"),
    ("clear", "clear the terminal screen"),
    ("list", "list objects"),
    ("ps", "List threads in the system"),
    ("free", "Show the memory usage in the system"),
    ("date", "get date and time or set (local timezone) [year month day hour min sec]"),
    ("ls", "List information about the FILEs."),
    ("cd", "Change the shell working directory."),
    ("pwd", "Print the name of the current working directory."),
    ("cat", "Concatenate FILE(s)"),
    ("cp", "Copy SOURCE to DEST."),
    ("mv", "Rename SOURCE to DEST."),
    ("rm", "Remove(unlink) the FILE(s)."),
    ("mkdir", "Create the DIRECTORY."),
    ("echo", "echo string to file"),
    ("ifconfig", "list the information of all network interfaces"),
    ("ping", "ping network host"),
    ("history", "show command history"),
    ("reboot", "reboot the cpu"),
    ("charset", "output UTF-8/GBK charset test bytes"),
    ("byte_send", "output fixed byte sequence for hex view"),
    ("env", "show environment variables"),
    ("setenv", "set environment variable"),
    ("unsetenv", "unset environment variable"),
    ("export", "export environment variable"),
    ("printenv", "print environment variable"),
    ("ip", "show ip address"),
    ("sx", "send file via XModem"),
    ("rx", "receive file via XModem"),
    ("sy", "send file via YModem"),
    ("ry", "receive file via YModem"),
    ("sz", "send file via ZModem"),
    ("rz", "receive file via ZModem"),
]


def _help_output():
    out = ["RT-Thread shell commands:"]
    for name, desc in COMMANDS:
        out.append(f"{name:<16} - {desc}")
    out.append("")
    return "\r\n".join(out) + "\r\n"


def _list_thread_output():
    header = ("thread   pri  status      sp     stack size max used left tick   error  "
              "tcb addr   usage")
    sep = ("-------- ---  ------- ---------- ----------  ------  ---------- ------- "
           "---------- -----")

    def row(name, pri, status, sp, stack, used, tick, tcb, usage):
        us = f" {usage:3d}%" if usage is not None else "  N/A"
        return (f"{name:<8} {pri:3d} {status} 0x{sp:08x} 0x{stack:08x}    "
                f"{used:02d}%   0x{tick:08x} OK     {tcb}{us}")

    lines = [header, sep]
    lines.append(row("main", 10, " ready  ", 0x40, 0x800, 14, 0x5, "0x20000400", 0))
    lines.append(row("tshell", 20, " running", 0x60, 0x800, 22, 0xa, "0x20000c00", None))
    lines.append(row("tidle", 31, " ready  ", 0x40, 0x100, 10, 0x3, "0x20000d00", None))
    lines.append(row("timer", 4, " suspend", 0x40, 0x200, 8, 0x9, "0x20000e00", None))
    return "\r\n".join(lines) + "\r\n"


def _list_device_output():
    header = "device           type         ref count"
    sep = "-------- -------------------- ----------"
    devices = [
        ("uart0", "Character Device", 1),
        ("uart1", "Character Device", 0),
        ("pin", "Pin Device", 0),
        ("i2c0", "I2C Bus", 0),
        ("spi0", "SPI Bus", 0),
    ]
    lines = [header, sep]
    for name, typ, ref in devices:
        lines.append(f"{name:<8} {typ:<20} {ref:<8}")
    return "\r\n".join(lines) + "\r\n"


def _list_timer_output():
    header = "timer    periodic   timeout    activated     mode"
    sep = "-------- ---------- ---------- ----------- ---------"
    row = f"{'timer1':<8} 0x00000010 0x00000010 activated   periodic"
    return "\r\n".join([header, sep, row, "current tick:0x00000000"]) + "\r\n"


FREE_OUTPUT = (
    "total    : 131072\r\n"
    "used     : 34816\r\n"
    "maximum  : 49152\r\n"
    "available: 96256\r\n"
)

IFCONFIG_OUTPUT = (
    "network interface device: e0 (Default)\r\n"
    "MTU: 1500\r\n"
    "MAC: 00 11 22 33 44 55 \r\n"
    "FLAGS: UP LINK_UP DHCP_DISABLE ETHARP BROADCAST IGMP\r\n"
    "ip address: 192.168.1.10\r\n"
    "gw address: 192.168.1.1\r\n"
    "net mask  : 255.255.255.0\r\n"
    "dns server #0: 192.168.1.1\r\n"
)

# 与 test-telnet-server.py 对齐的字符集测试字节
CHARSET_UTF8 = "你好，RT-Thread！串口会话测试 123\r\n".encode("utf-8")
try:
    CHARSET_GBK = "你好，RT-Thread！串口会话测试 123\r\n".encode("gbk")
except UnicodeEncodeError:  # pragma: no cover
    CHARSET_GBK = b""

# 固定字节序列，用于 HEX 视图验证：直观可辨认的 0x00-0x1F 控制字 + ASCII
BYTE_DEMO = (
    bytes(range(0x00, 0x10))
    + b"\r\nHEX-VIEW-DEMO: 0123456789ABCDEF\r\n"
    + bytes(range(0x80, 0x90))
    + b"\r\nBYTE-DEMO-END\r\n"
)

# 虚拟文件系统：初始内容（含中文，用于 cat/字符集测试）
DEFAULT_FS = {
    "/": ["bin", "etc", "dev", "mnt", "root", "tmp", "usr"],
    "/bin": [],
    "/etc": ["motd", "version.txt"],
    "/etc/motd": "Welcome to RT-Thread Virtual Device!\r\n",
    "/etc/version.txt": f"RT-Thread {FINSH_VERSION}\r\n",
    "/root": ["hello.txt"],
    "/root/hello.txt": "Hello from RT-Thread msh!\r\n你好，FinSH。\r\n",
    "/tmp": [],
    "/usr": ["bin"],
    "/usr/bin": ["demo"],
    "/mnt": [],
    "/dev": ["uart0", "uart1", "console"],
}


class FinshShell:
    """RT-Thread FinSH(msh) 交互 Shell 状态机。"""

    def __init__(self, device):
        self.device = device  # RTTDevice，用于触发传输与直接串口写
        self.cwd = "/"
        self.env = {
            "HOSTNAME": "rt-thread",
            "PWD": "/",
            "PATH": "/bin:/usr/bin",  # 简化
            "HOME": "/root",
            "TERM": "vt100",
        }
        self.ip = "192.168.1.10"

    # ── 输出辅助 ──────────────────────────────────────────────
    def _fs_norm(self, path):
        """归一化虚拟文件系统路径。"""
        if not path.startswith("/"):
            path = self.cwd.rstrip("/") + "/" + path
        parts = []
        for seg in path.split("/"):
            if seg in ("", "."):
                continue
            if seg == "..":
                if parts:
                    parts.pop()
            else:
                parts.append(seg)
        return "/" + "/".join(parts)

    def _fs_get(self, path):
        norm = self._fs_norm(path)
        if norm in DEFAULT_FS and isinstance(DEFAULT_FS[norm], str):
            return DEFAULT_FS[norm]
        return None

    def _fs_is_dir(self, path):
        norm = self._fs_norm(path)
        if norm in DEFAULT_FS and isinstance(DEFAULT_FS[norm], list):
            return True
        return False

    def _fs_join_dir(self, dirpath):
        d = self._fs_norm(dirpath)
        if d not in DEFAULT_FS:
            DEFAULT_FS[d] = []
        return d

    # ── 命令分发 ──────────────────────────────────────────────
    def handle(self, cmdline: str) -> bytes:
        """处理一行命令，返回应输出的字节（不含末尾补的 \r\n，若需换行请在输出内带 \\r\\n）。"""
        c = cmdline.strip()
        if not c:
            return b""

        parts = shlex.split(c)
        name = parts[0].lower()
        args = parts[1:]

        handler = getattr(self, f"cmd_{name}", None)
        if handler is None:
            # 常见别名/单字母简写
            alias = {"h": "help", "uname": "version", "mem": "free",
                     "cls": "clear", "byte_send": "bytesend"}
            handler = getattr(self, "cmd_" + alias[name], None) if name in alias else None
            if handler is None:
                # msh 对无法直接识别的字符串，尝试当作可执行/脚本名
                if self.fs_executable(c):
                    return self.fs_run(c, args)
                return f"{parts[0]}: command not found.\r\n".encode("utf-8")
        try:
            out = handler(args)
        except Exception as e:  # 模拟设备命令异常
            out = f"msh: {name}: error: {e}\r\n"
        return out.encode("utf-8") if isinstance(out, str) else out

    def fs_executable(self, cmd):
        first = shlex.split(cmd)[0] if shlex.split(cmd) else ""
        return first in ("demo", "hello", "app")

    def fs_run(self, cmd, args):
        first = shlex.split(cmd)[0]
        if first == "demo":
            return "demo: running virtual app... done.\r\n"
        return f"msh: {first}: not found\r\n"

    # ── 基础命令 ──────────────────────────────────────────────
    def cmd_help(self, args):
        return _help_output()

    def cmd_version(self, args):
        return _build_banner()

    def cmd_ps(self, args):
        return _list_thread_output()

    def cmd_free(self, args):
        return FREE_OUTPUT

    def cmd_date(self, args):
        if len(args) >= 6:  # date [year month day hour min sec]
            return f"old: {_c_ctime()}\r\nnow: {_c_ctime()}\r\n"
        if args:
            return ("please input: date [year month day hour min sec] or date\r\n"
                    "e.g: date 2018 01 01 23 59 59 or date\r\n")
        return (f"local time: {_c_ctime()}\r\n"
                f"timestamps: {int(time.time())}\r\n"
                "timezone: UTC+08:00:00\r\n")

    def cmd_list(self, args):
        usage = ("Usage: list [options]\r\n"
                 "[options]:\r\n"
                 "    thread        - list threads\r\n"
                 "    timer         - list timers\r\n"
                 "    device        - list devices\r\n")
        if not args:
            return usage
        sub = args[0].lower()
        if sub == "thread":
            return _list_thread_output()
        if sub == "timer":
            return _list_timer_output()
        if sub == "device":
            return _list_device_output()
        return usage

    # ── 文件系统 ──────────────────────────────────────────────
    def cmd_pwd(self, args):
        return self.cwd + "\r\n"

    def cmd_cd(self, args):
        if not args:
            return self.cwd + "\r\n"
        norm = self._fs_norm(args[0])
        if self._fs_is_dir(norm):
            self.cwd = norm
            self.env["PWD"] = norm
            return b""
        return f"No such directory: {args[0]}\r\n"

    def cmd_ls(self, args):
        target = args[0] if args else self.cwd
        norm = self._fs_norm(target)
        if not self._fs_is_dir(norm):
            return "No such directory\r\n"
        out = f"Directory {norm}:\r\n"
        for e in DEFAULT_FS.get(norm, []):
            full = norm.rstrip("/") + "/" + e
            if isinstance(DEFAULT_FS.get(full), list):
                out += f"{e:<20}{'<DIR>':<25}\r\n"
            else:
                size = len(DEFAULT_FS.get(full, "").encode("utf-8"))
                out += f"{e:<20}{size:<25}\r\n"
        return out

    def cmd_mkdir(self, args):
        if not args:
            return ("Usage: mkdir [OPTION] DIRECTORY\r\n"
                    "Create the DIRECTORY, if they do not already exist.\r\n")
        self._fs_join_dir(args[0])
        return b""

    def cmd_cat(self, args):
        if not args:
            return "Usage: cat [FILE]...\r\nConcatenate FILE(s)\r\n"
        content = self._fs_get(args[0])
        if content is None:
            return f"Open {args[0]} failed\r\n"
        return content

    def cmd_echo(self, args):
        if len(args) == 1:
            return args[0] + "\r\n"
        if len(args) == 2:
            DEFAULT_FS[self._fs_norm(args[1])] = args[0]
            return b""
        return "Usage: echo \"string\" [filename]\r\n"

    def cmd_rm(self, args):
        if not args:
            return "Usage: rm option(s) FILE...\r\nRemove (unlink) the FILE(s).\r\n"
        norm = self._fs_norm(args[0])
        if norm in DEFAULT_FS:
            if isinstance(DEFAULT_FS[norm], list):
                return f"cannot remove '{args[0]}': Is a directory\r\n"
            del DEFAULT_FS[norm]
            return b""
        return f"cannot remove '{args[0]}': No such file or directory\r\n"

    def cmd_cp(self, args):
        if len(args) != 2:
            return "Usage: cp SOURCE DEST\r\nCopy SOURCE to DEST.\r\n"
        content = self._fs_get(args[0])
        if content is None:
            return f"Read {args[0]} failed\r\n"
        DEFAULT_FS[self._fs_norm(args[1])] = content
        return b""

    def cmd_mv(self, args):
        if len(args) != 2:
            return ("Usage: mv SOURCE DEST\r\n"
                    "Rename SOURCE to DEST, or move SOURCE(s) to DIRECTORY.\r\n")
        content = self._fs_get(args[0])
        if content is None:
            return f"Read {args[0]} failed\r\n"
        src = self._fs_norm(args[0])
        dst = self._fs_norm(args[1])
        DEFAULT_FS[dst] = content
        if src != dst:
            del DEFAULT_FS[src]
        return f"{args[0]} => {args[1]}\r\n"

    # ── 环境变量 ──────────────────────────────────────────────
    def cmd_env(self, args):
        return "".join(f"{k}={v}\r\n" for k, v in self.env.items())

    def cmd_setenv(self, args):
        if len(args) < 2:
            return "setenv: VAR VALUE\r\n"
        self.env[args[0]] = args[1]
        return b""

    def cmd_export(self, args):
        return self.cmd_setenv(args)

    def cmd_unsetenv(self, args):
        if not args:
            return "unsetenv: VAR\r\n"
        self.env.pop(args[0], None)
        return b""

    def cmd_printenv(self, args):
        if not args:
            return self.cmd_env(args)
        return f"{args[0]}={self.env.get(args[0], '')}\r\n"

    # ── 网络 ──────────────────────────────────────────────────────────
    def cmd_ifconfig(self, args):
        return IFCONFIG_OUTPUT

    def cmd_ip(self, args):
        if args and args[0] == "addr":
            return "inet addr: 192.168.1.10  Mask: 255.255.255.0\r\n"
        return "Usage: ip addr\r\n"

    def cmd_ping(self, args):
        if not args:
            return "Please input: ping <host address>\r\n"
        host = args[0]
        lines = []
        for seq in range(4):
            lines.append(f"64 bytes from {host} icmp_seq={seq} ttl=64 time={seq % 2} ms")
        return "\r\n".join(lines) + "\r\n"

    # ── 其他 ──────────────────────────────────────────────────────────
    def cmd_clear(self, args):
        return "\x1b[2J\x1b[H"  # 模拟 ANSI 清屏（HEX 视图下可见 ESC 序列）

    def cmd_reboot(self, args):
        self.device.reboot()
        return b""

    def cmd_history(self, args):
        return "".join(f"{i + 1:3d}  {c}\r\n"
                       for i, c in enumerate(self.device.history))

    def cmd_bytesend(self, args):
        return BYTE_DEMO

    def cmd_charset(self, args):
        return b"charset(UTF-8): " + CHARSET_UTF8 + b"charset(GBK): " + CHARSET_GBK

    # ── 传输命令 ──────────────────────────────────────────────
    def cmd_sx(self, args):
        """设备向上位机发送文件（XModem）。"""
        return self.device.transfer("x", "send", args)

    def cmd_rx(self, args):
        """设备从上位机接收文件（XModem）。"""
        return self.device.transfer("x", "recv", args)

    def cmd_sy(self, args):
        return self.device.transfer("y", "send", args)

    def cmd_ry(self, args):
        return self.device.transfer("y", "recv", args)

    def cmd_sz(self, args):
        return self.device.transfer("z", "send", args)

    def cmd_rz(self, args):
        return self.device.transfer("z", "recv", args)

    # ── 提示符 ──────────────────────────────────────────────
    def prompt(self) -> bytes:
        cwd = self.cwd if self.cwd else "/"
        return f"msh {cwd}>".encode("utf-8")


# ─────────────────────────────────────────────────────────────────────────────
# 自动应答规则（模拟 Lua / 脚本驱动）
# ─────────────────────────────────────────────────────────────────────────────

def load_rules(path):
    """解析规则文件：每行 `REGEX<TAB>RESPONSE`，# 开头为注释。

    收到的一行字符串若匹配 REGEX，则发送 RESPONSE（可含 \\r\\n）。
    返回 [(编译后的regex, response_bytes)]。
    """
    rules = []
    with open(path, "r", encoding="utf-8") as f:
        for raw in f:
            line = raw.rstrip("\r\n")
            if not line or line.lstrip().startswith("#"):
                continue
            if "\t" in line:
                pat, resp = line.split("\t", 1)
            else:
                # 兼容空格分隔但尽量容忍
                parts = line.split(" ", 1)
                if len(parts) < 2:
                    continue
                pat, resp = parts[0], parts[1]
            try:
                rules.append((re.compile(pat), resp.encode("utf-8")))
            except re.error as e:
                print(f"[警告] 规则正则无效，已跳过: {pat} ({e})")
    return rules


# ─────────────────────────────────────────────────────────────────────────────
# ZModem 标准实现（独立 oracle，测试 TauTerm 正确性）
# ─────────────────────────────────────────────────────────────────────────────

ZPAD = 0x2A      # '*' 帧起始
ZDLE = 0x18      # Ctrl-X 转义
ZHEX = 0x42      # 'B' 十六进制帧头
ZBIN = 0x41      # 'A' 二进制帧头（CRC-16）
ZBIN32 = 0x43    # 'C' 二进制帧头（CRC-32）

ZRQINIT = 0x00
ZRINIT = 0x01
ZSINIT = 0x02
ZACK = 0x03
ZFILE = 0x04
ZSKIP = 0x05
ZNAK = 0x06
ZABORT = 0x07
ZFIN = 0x08
ZRPOS = 0x09
ZDATA = 0x0A
ZEOF = 0x0B
ZFERR = 0x0C
ZCRC = 0x0D

ZCRCE = 0x68  # 'h' 帧结束，下一帧头紧随（无错不响应）
ZCRCG = 0x69  # 'i' 帧继续
ZCRCQ = 0x6A  # 'j' 帧继续，期望 ZACK
ZCRCW = 0x6B  # 'k' 帧结束，期望 ZACK

CANFDX = 0x01
CANOVIO = 0x02
CANBRK = 0x04
CANFC32 = 0x20

# 接收方能力（本实现支持 CRC32 与全双工）
_ZMODEM_CAPS = CANFDX | CANOVIO | CANBRK | CANFC32
# ZRINIT 的四个 info 字节：flags 帧按 F3 F2 F1 F0 传输，ZF0 为能力标志
_ZMODEM_RINIT_INFO = bytes([0, 0, 0, _ZMODEM_CAPS])

# 发送时需要 ZDLE 转义的控制字节（与 lrzsz 一致）
_ZMODEM_ESCAPE = (ZDLE, 0x10, 0x11, 0x13, 0x0D, 0x0A)


def _zmodem_crc16(data):
    """CCITT CRC-16（多项式 0x1021，初值 0）。"""
    return binascii.crc_hqx(data, 0)


def _zmodem_crc32(data):
    """IEEE 802.3 CRC-32（反射多项式 0xEDB88320）。"""
    return binascii.crc32(data) & 0xFFFFFFFF


def _zmodem_escape(data):
    """对二进制数据进行 ZDLE 转义（控制字节前插 ZDLE 并异或 0x40）。"""
    out = bytearray()
    for b in data:
        if b in _ZMODEM_ESCAPE:
            out.append(ZDLE)
            out.append(b ^ 0x40)
        else:
            out.append(b)
    return bytes(out)


class _ZmodemStream:
    """包装 getc(size, timeout) 的带缓冲字节读取器。"""

    def __init__(self, getc, timeout=5):
        self.getc = getc
        self.timeout = timeout
        self.buf = b""

    def read_byte(self):
        if not self.buf:
            chunk = self.getc(256, self.timeout)
            if not chunk:
                return None
            self.buf = chunk
        b = self.buf[:1]
        self.buf = self.buf[1:]
        return b[0]


class ZModem:
    """标准 ZModem 单文件收发（receive=rz / send=sz）。"""

    def __init__(self, getc, putc, log=None):
        self.getc = getc
        self.putc = putc
        self.log = log or (lambda *a: None)
        self.stream = _ZmodemStream(getc)

    # ── 帧发送 ──────────────────────────────────────────────
    def _send_hex_header(self, ftype, info4):
        body = bytes([ftype]) + bytes(info4)
        crc = _zmodem_crc16(body)
        frame = (bytes([ZPAD, ZPAD, ZDLE, ZHEX])
                 + body.hex().encode("ascii")
                 + format(crc, "04x").encode("ascii")
                 + b"\r\n")
        if ftype not in (ZACK, ZFIN):
            frame += b"\x11"  # XON（ZACK/ZFIN 后不发送）
        self.putc(frame)

    def _send_bin_header(self, ftype, info4, crc32):
        body = bytes([ftype]) + bytes(info4)
        if crc32:
            crc_bytes = struct.pack(">I", _zmodem_crc32(body))
            intro = bytes([ZPAD, ZDLE, ZBIN32])
        else:
            crc_bytes = struct.pack(">H", _zmodem_crc16(body))
            intro = bytes([ZPAD, ZDLE, ZBIN])
        self.putc(intro + _zmodem_escape(body + crc_bytes))

    def _send_data_subpacket(self, data, end_type, crc32):
        crc = (_zmodem_crc32 if crc32 else _zmodem_crc16)(data + bytes([end_type]))
        crc_bytes = struct.pack(">I" if crc32 else ">H", crc)
        out = bytearray(_zmodem_escape(data))
        out.append(ZDLE)
        out.append(end_type)
        out += _zmodem_escape(crc_bytes)
        self.putc(bytes(out))

    # ── 帧接收 ──────────────────────────────────────────────
    def _read_header(self):
        """读取一个帧头，返回 (帧类型, info4字节, 是否CRC32)。"""
        while True:
            b = self.stream.read_byte()
            if b is None:
                raise TimeoutError("等待 ZMODEM 帧头超时")
            if b != ZPAD:
                continue
            nxt = self.stream.read_byte()
            if nxt is None:
                raise TimeoutError("ZMODEM 帧头不完整")
            if nxt == ZDLE:
                style = self.stream.read_byte()
                if style == ZBIN:
                    return self._read_bin_header(crc32=False)
                if style == ZBIN32:
                    return self._read_bin_header(crc32=True)
                continue
            if nxt == ZPAD:
                n2 = self.stream.read_byte()
                if n2 == ZDLE:
                    n3 = self.stream.read_byte()
                    if n3 == ZHEX:
                        return self._read_hex_header()

    def _read_bin_header(self, crc32):
        n = 5 + (4 if crc32 else 2)
        raw = bytearray()
        while len(raw) < n:
            b = self.stream.read_byte()
            if b is None:
                raise TimeoutError("二进制帧头不完整")
            if b == ZDLE:
                nb = self.stream.read_byte()
                if nb is None:
                    raise TimeoutError("二进制帧头转义不完整")
                raw.append(nb ^ 0x40)
            else:
                raw.append(b)
        body = bytes(raw[:5])
        crc_bytes = bytes(raw[5:])
        expected = (_zmodem_crc32 if crc32 else _zmodem_crc16)(body)
        got = int.from_bytes(crc_bytes, "big")
        if expected != got:
            raise ValueError("二进制帧头 CRC 校验失败")
        return body[0], body[1:5], crc32

    def _read_hex_header(self):
        hexchars = bytearray()
        while True:
            b = self.stream.read_byte()
            if b is None:
                raise TimeoutError("十六进制帧头不完整")
            if b in (0x0D, 0x0A):
                break
            hexchars.append(b)
        s = bytes(hexchars).decode("ascii", "replace")
        if len(s) < 14:
            raise ValueError("十六进制帧头过短")
        body = bytes.fromhex(s[:10])
        crc = int(s[10:14], 16)
        if _zmodem_crc16(body) != crc:
            raise ValueError("十六进制帧头 CRC 校验失败")
        return body[0], body[1:5], False

    def _read_data_subpacket(self, crc32, writer):
        data = bytearray()
        end_type = None
        while True:
            b = self.stream.read_byte()
            if b is None:
                raise TimeoutError("数据子包读取超时")
            if b == ZDLE:
                nb = self.stream.read_byte()
                if nb is None:
                    raise TimeoutError("数据子包转义不完整")
                if nb in (ZCRCE, ZCRCG, ZCRCQ, ZCRCW):
                    end_type = nb
                    break
                data.append(nb ^ 0x40)
            else:
                data.append(b)
        crc_len = 4 if crc32 else 2
        crc_bytes = bytearray()
        while len(crc_bytes) < crc_len:
            b = self.stream.read_byte()
            if b is None:
                raise TimeoutError("子包 CRC 不完整")
            if b == ZDLE:
                nb = self.stream.read_byte()
                crc_bytes.append(nb ^ 0x40)
            else:
                crc_bytes.append(b)
        raw = bytes(data)
        expected = (_zmodem_crc32 if crc32 else _zmodem_crc16)(raw + bytes([end_type]))
        got = int.from_bytes(bytes(crc_bytes), "big")
        if expected != got:
            raise ValueError("数据子包 CRC 校验失败")
        if writer:
            writer(raw)
        return len(raw), end_type

    # ── rz：接收文件（设备为接收方） ────────────────────────
    def receive(self, target):
        self.log("[ZModem] 开始接收 (rz)")
        self._send_hex_header(ZRINIT, _ZMODEM_RINIT_INFO)
        while True:
            ftype, info4, crc32 = self._read_header()
            if ftype == ZFILE:
                name_info, _ = self._read_data_subpacket(crc32, None)
                sender_name = name_info.split(b"\x00", 1)[0].decode("utf-8", "replace")
                self.log(f"[ZModem] 接收文件: {sender_name}")
                break
            if ftype == ZFIN:
                return False, "对方未发送文件即结束会话"
            if ftype == ZRINIT:
                self._send_hex_header(ZRINIT, _ZMODEM_RINIT_INFO)
        self._send_hex_header(ZRPOS, struct.pack("<I", 0))
        total = 0
        with open(target, "wb") as f:
            while True:
                ftype, info4, crc32 = self._read_header()
                if ftype == ZDATA:
                    while True:
                        n, et = self._read_data_subpacket(crc32, f.write)
                        total += n
                        if et == ZCRCW:
                            self._send_hex_header(ZACK, struct.pack("<I", total))
                            break
                        if et == ZCRCE:
                            break
                elif ftype == ZEOF:
                    final = struct.unpack("<I", info4)[0]
                    self.log(f"[ZModem] 文件结束：接收 {total} 字节（ZEOF 偏移 {final}）")
                    self._send_hex_header(ZRINIT, _ZMODEM_RINIT_INFO)
                    break
                elif ftype == ZFIN:
                    break
        try:
            ftype, _, _ = self._read_header()
            if ftype == ZFIN:
                self._send_hex_header(ZFIN, bytes(4))
                self.stream.read_byte()  # 读 OO
                self.stream.read_byte()
        except Exception:
            pass
        return True, f"已接收 {total} 字节"

    # ── sz：发送文件（设备为发送方） ────────────────────────
    def send(self, target):
        self.log("[ZModem] 开始发送 (sz)")
        if not os.path.isfile(target):
            return False, f"文件不存在: {target}"
        size = os.path.getsize(target)
        self._send_hex_header(ZRQINIT, bytes(4))
        while True:
            ftype, info4, _ = self._read_header()
            if ftype == ZRINIT:
                break
            if ftype == ZFIN:
                return False, "接收方未就绪"
        use_crc32 = bool(info4[3] & CANFC32)
        name = os.path.basename(target)
        mtime = int(os.path.getmtime(target))
        file_info = (name.encode("utf-8", "replace") + b"\x00"
                     + str(size).encode() + b" " + format(mtime, "o").encode() + b"\x00")
        self._send_bin_header(ZFILE, bytes(4), use_crc32)
        self._send_data_subpacket(file_info, ZCRCW, use_crc32)
        while True:
            ftype, _, _ = self._read_header()
            if ftype == ZRPOS:
                break
            if ftype in (ZSKIP, ZFIN):
                return False, "接收方跳过/结束文件"
        offset = 0
        with open(target, "rb") as f:
            while True:
                chunk = f.read(1024)
                if not chunk:
                    break
                self._send_bin_header(ZDATA, struct.pack("<I", offset), use_crc32)
                self._send_data_subpacket(chunk, ZCRCW, use_crc32)
                ftype, _, _ = self._read_header()
                if ftype != ZACK:
                    return False, f"期望 ZACK，收到 {ftype:#x}"
                offset += len(chunk)
        self._send_bin_header(ZEOF, struct.pack("<I", offset), use_crc32)
        ftype, _, _ = self._read_header()
        if ftype != ZRINIT:
            return False, f"期望 ZRINIT，收到 {ftype:#x}"
        self._send_hex_header(ZFIN, bytes(4))
        ftype, _, _ = self._read_header()
        if ftype != ZFIN:
            return False, f"期望 ZFIN，收到 {ftype:#x}"
        self.putc(b"OO")
        return True, f"已发送 {offset} 字节"


# RT-Thread 设备仿真（串口交互主体）
# ─────────────────────────────────────────────────────────────────────────────

class RTTDevice:
    def __init__(self, port, baud, hex_dump=False, rules_path=None,
                 quiet_banner=False, log_path=None):
        self.port = port
        self.baud = baud
        self.hex_dump = hex_dump
        self.quiet_banner = quiet_banner
        self.rules = load_rules(rules_path) if rules_path else []
        self.log_fp = open(log_path, "a", encoding="utf-8") if log_path else None

        self.ser = serial.Serial(port, baud, timeout=0.03, write_timeout=0.5)
        self.shell = FinshShell(self)
        self.running = True

        # 行编辑器状态（对应 FinSH shell.c 的逐字符处理状态机）
        self.line = bytearray()
        self.curpos = 0
        self.history = deque(maxlen=5)   # FINSH_HISTORY_LINES
        self.hist_index = None

    # ── 日志 ────────────────────────────────────────────────
    def _log(self, text):
        print(text)
        if self.log_fp:
            self.log_fp.write(text + "\n")
            self.log_fp.flush()

    def _dump(self, direction, data):
        if not self.hex_dump:
            return
        hexs = " ".join(f"{b:02X}" for b in data)
        ascii_repr = "".join(chr(b) if 0x20 <= b < 0x7F else "." for b in data)
        self._log(f"  [{direction}] {len(data):3d}B  {hexs:<64} |{ascii_repr}|")

    # ── 发送 ────────────────────────────────────────────────
    def send(self, data):
        code = data if isinstance(data, bytes) else data.encode("utf-8")
        self._dump("TX", code)
        try:
            self.ser.write(code)
            self.ser.flush()
        except serial.SerialTimeoutException:
            # com0com 为 null-modem：对端（TauTerm）未连接时写缓冲无法排空，
            # write_timeout 到期会抛出此异常，由调用方决定是否重试。
            return False
        time.sleep(0.02)
        return True

    def send_banner(self, with_prompt=True):
        ok = True
        if not self.quiet_banner:
            ok = self.send(_build_banner()) and ok
        if with_prompt:
            ok = self.send(self.shell.prompt()) and ok
        return ok

    def _peer_connected(self):
        # com0com 为 null-modem 接线：对端 DTR 会映射为本端 DSR/DCD。
        try:
            return bool(self.ser.cd or self.ser.dsr)
        except Exception:
            return False

    # ── 复位 ────────────────────────────────────────────────
    def reboot(self):
        self._log("[设备] 复位...")
        self.send("\r\n===== system reset =====\r\n")
        time.sleep(0.3)
        # 只发横幅，提示符由 _dispatch 统一补发，避免与 cmd_reboot 重复
        self.send_banner(with_prompt=False)

    # ── 主循环 ──────────────────────────────────────────────
    def run(self):
        self._log(f"[*] RT-Thread 设备仿真已启动: {self.port} @ {self.baud}")
        self._log(f"[*] 请在 TauTerm 中连接对端端口; 输入 Ctrl-C 退出本脚本")

        banner_sent = False
        warned_waiting = False
        last_banner_attempt = 0.0
        buf = b""
        while self.running:
            try:
                data = self.ser.read(256)
            except serial.SerialException as e:
                self._log(f"[错误] 串口异常: {e}")
                break

            # 对端（TauTerm）连接前，com0com 写缓冲无法排空会触发写超时。
            # 等对端上线（DSR/DCD 置位）后再发启动横幅；未就绪时周期性兜底尝试。
            if not banner_sent:
                now = time.time()
                if self._peer_connected() or now - last_banner_attempt >= 0.5:
                    last_banner_attempt = now
                    if self.send_banner():
                        banner_sent = True
                if not banner_sent and not data:
                    if not warned_waiting:
                        self._log("[*] 等待 TauTerm 连接对端端口后发送启动横幅...")
                        warned_waiting = True
                    time.sleep(0.05)
                    continue

            if not data:
                continue

            buf += data
            buf = self._process_bytes(buf)

        self.cleanup()

    # ── 行编辑器（对应 FinSH shell.c 的逐字符处理） ───────────────────
    def _parse_escape(self, buf, i):
        """解析 buf[i] 处的转义序列，返回 (动作, 消费长度)；序列不完整返回 (None, 0)。"""
        n = len(buf)
        if i + 1 >= n:
            return None, 0
        nxt = buf[i + 1]
        if nxt == 0x5B:  # CSI '[' 序列
            if i + 2 >= n:
                return None, 0
            c = buf[i + 2]
            if c in (0x41, 0x42, 0x43, 0x44):  # A/B/C/D = 上/下/右/左
                return {0x41: "up", 0x42: "down", 0x43: "right", 0x44: "left"}[c], 3
            if c == 0x48:
                return "home", 3
            if c == 0x46:
                return "end", 3
            if c in (0x31, 0x32, 0x33, 0x34):  # 1~/2~/3~/4~ = home/insert/del/end
                j = i + 3
                while j < n and buf[j] != 0x7E:
                    j += 1
                if j >= n:
                    return None, 0
                return {0x31: "home", 0x32: "insert", 0x33: "del", 0x34: "end"}[c], j - i + 1
            j = i + 3
            while j < n and not (0x40 <= buf[j] <= 0x7E):
                j += 1
            if j >= n:
                return None, 0
            return "ignore", j - i + 1
        if nxt == 0x4F:  # SS3 'O' 序列
            if i + 2 >= n:
                return None, 0
            c = buf[i + 2]
            return {0x41: "up", 0x42: "down", 0x43: "right", 0x44: "left",
                    0x48: "home", 0x46: "end"}.get(c, "ignore"), 3
        # 裸 ESC 或 Alt+键：一并吞 2 字节
        return "ignore", min(2, n - i)

    def _do_escape_action(self, action):
        if action == "up":
            self._history_up()
        elif action == "down":
            self._history_down()
        elif action == "right":
            if self.curpos < len(self.line):
                self.send(bytes([self.line[self.curpos]]))
                self.curpos += 1
        elif action == "left":
            if self.curpos > 0:
                self.send(b"\b")
                self.curpos -= 1
        elif action == "home":
            while self.curpos > 0:
                self.send(b"\b")
                self.curpos -= 1
        elif action == "end":
            while self.curpos < len(self.line):
                self.send(bytes([self.line[self.curpos]]))
                self.curpos += 1
        elif action == "del":
            self._delete_char()
        # "insert" / "ignore"：不处理

    def _process_bytes(self, buf):
        i = 0
        n = len(buf)
        while i < n:
            b = buf[i]
            if b == 0x1B:
                action, consumed = self._parse_escape(buf, i)
                if action is None:
                    return buf[i:]  # 序列不完整，等下一包
                self._dump("RCV-ESC", buf[i:i + consumed])
                self._do_escape_action(action)
                i += consumed
                continue
            if b == 0x03:  # Ctrl-C：真实 FinSH 无特殊处理，按普通字节对待
                self._insert_char(b)
                self._dump("RCV", bytes([b]))
                i += 1
                continue
            if b in (0x7F, 0x08):  # 退格
                self._backspace()
                self._dump("RCV", bytes([b]))
                i += 1
                continue
            if b == 0x09:  # Tab 补全
                self._tab_complete()
                self._dump("RCV", bytes([b]))
                i += 1
                continue
            if b == 0x17:  # Ctrl-W 删词
                self._ctrl_w()
                self._dump("RCV", bytes([b]))
                i += 1
                continue
            if b in (0x0D, 0x0A):  # 回车/换行：提交命令
                self._submit_line()
                self._dump("RCV", bytes([b]))
                i += 1
                continue
            # 可打印/任意字节：本地回显
            self._insert_char(b)
            self._dump("RCV", bytes([b]))
            i += 1
        return b""

    # ── 行编辑动作 ─────────────────────────────────────────────────────
    def _insert_char(self, b):
        if self.curpos < len(self.line):
            self.line.insert(self.curpos, b)
            self.curpos += 1
            tail = bytes(self.line[self.curpos - 1:])
            self.send(tail)
            if len(tail) > 1:
                self.send(b"\033[%dD" % (len(tail) - 1))
        else:
            self.line.append(b)
            self.curpos += 1
            self.send(bytes([b]))

    def _repaint_tail(self):
        """从当前光标处擦除到行尾并重绘，光标回到 self.curpos。"""
        tail = bytes(self.line[self.curpos:])
        self.send(b"\033[K")
        if tail:
            self.send(tail)
            self.send(b"\033[%dD" % len(tail))

    def _backspace(self):
        if self.curpos == 0:
            return
        self.curpos -= 1
        del self.line[self.curpos]
        if self.curpos == len(self.line):
            self.send(b"\b \b")
        else:
            self.send(b"\b")
            self._repaint_tail()

    def _delete_char(self):
        if self.curpos < len(self.line):
            del self.line[self.curpos]
            self._repaint_tail()

    def _ctrl_w(self):
        if self.curpos == 0:
            return
        start = self.curpos
        while start > 0 and self.line[start - 1] in (0x20, 0x09):
            start -= 1
        while start > 0 and self.line[start - 1] not in (0x20, 0x09):
            start -= 1
        del_count = self.curpos - start
        del self.line[start:self.curpos]
        self.curpos = start
        self.send(b"\033[%dD" % del_count)
        self._repaint_tail()

    def _replace_line(self, text):
        """整体重绘一行（历史导航用）：擦除当前行，重打提示符 + 新内容。"""
        self.send(b"\033[2K\r" + self.shell.prompt() + text.encode("utf-8"))
        self.line[:] = text.encode("utf-8")
        self.curpos = len(self.line)

    def _history_up(self):
        if not self.history:
            return
        if self.hist_index is None:
            self.hist_index = len(self.history) - 1
        else:
            self.hist_index = max(0, self.hist_index - 1)
        self._replace_line(self.history[self.hist_index])

    def _history_down(self):
        if self.hist_index is None:
            return
        self.hist_index += 1
        if self.hist_index >= len(self.history):
            self.hist_index = None
            self._replace_line("")
        else:
            self._replace_line(self.history[self.hist_index])

    def _tab_complete(self):
        self.send(b"\b" * self.curpos)
        self.curpos = 0
        line_str = bytes(self.line).decode("utf-8", "replace")
        word = line_str.split(" ", 1)[0]
        names = [n for n, _ in COMMANDS]
        matches = names if word == "" else [n for n in names if n.startswith(word)]
        self.send(b"\r\n")
        for m in matches:
            self.send(m.encode("utf-8") + b"\r\n")
        common = os.path.commonprefix(matches) if matches else word
        self.line[:] = common.encode("utf-8")
        self.curpos = len(self.line)
        self.send(self.shell.prompt() + bytes(self.line))

    def _submit_line(self):
        line_str = bytes(self.line).decode("utf-8", "replace")
        if line_str and (not self.history or self.history[-1] != line_str):
            self.history.append(line_str)
        self.hist_index = None
        self.send(b"\r\n")
        if line_str:
            self._dispatch(line_str)
        else:
            self.send(self.shell.prompt())
        self.line.clear()
        self.curpos = 0

    def _dispatch(self, line_str):
        self._log(f"[输入] {line_str}")

        # 自动应答规则（规则优先于内置命令，模拟 Lua/脚本驱动的设备）
        if self.rules:
            for pat, resp in self.rules:
                if pat.search(line_str):
                    self._log(f"[规则] 匹配 {pat.pattern} -> 应答 {len(resp)}B")
                    self.send(resp)
                    self.send(self.shell.prompt())
                    return

        # 正常命令分发
        out = self.shell.handle(line_str)
        if out:
            self.send(out)
            # 传输类命令的结果可能已包含换行并直接在设备侧完成，不再重复回车
            if not line_str.lower().startswith(("sx ", "rx ", "sy ", "ry ", "sz ", "rz ")):
                if not out.endswith(b"\r\n"):
                    self.send(b"\r\n")
        self.send(self.shell.prompt())

    def cleanup(self):
        self._log("[*] 关闭串口...")
        try:
            self.ser.close()
        except Exception:
            pass
        if self.log_fp:
            self.log_fp.close()

    # ── 文件传输 ─────────────────────────────────────────────
    def transfer(self, proto, direction, args):
        """响应 FinSH 传输命令。proto ∈ {x,y,z}; direction ∈ {send, recv}。"""
        if not args:
            return f"{direction}({proto}): missing file operand\r\n"
        target = args[0]

        # 直接串口读写的适配器
        def getc(size=1, timeout=5):
            self.ser.timeout = timeout
            return self.ser.read(size)

        def putc(data, timeout=5):
            self.ser.write(data)
            self.ser.flush()
            return len(data)

        print(f"[传输] {proto.upper()} {direction} {target} 开始")
        try:
            status, detail = self._run_transfer(proto, direction, target, getc, putc)
        finally:
            # 恢复传输前的主循环读超时，避免空闲 read 阻塞过久
            self.ser.timeout = 0.03
        if status:
            return (f"{proto.upper()} {direction} 成功: {detail}\r\n" if detail
                    else f"{proto.upper()} {direction} 成功\r\n")
        return f"{proto.upper()} {direction} 失败: {detail}\r\n"

    def _run_transfer(self, proto, direction, target, getc, putc):
        try:
            if direction == "recv":
                return self._transfer_recv(proto, target, getc, putc)
            return self._transfer_send(proto, target, getc, putc)
        except ImportError as e:
            return False, f"缺少库: {e}。请 pip install {self._pip_name(proto)} 后重试，或改用内置方案。"
        except Exception as e:
            return False, f"{type(e).__name__}: {e}"

    def _pip_name(self, proto):
        return {"x": "xmodem", "y": "ymodem"}.get(proto, "zmodem")

    def _transfer_send(self, proto, target, getc, putc):
        """设备向上位机发送文件（设备是 SENDER）。"""
        if not os.path.isfile(target):
            return False, f"文件不存在: {target}"

        if proto == "x":
            import xmodem

            modem = xmodem.XMODEM(getc, putc)
            with open(target, "rb") as fp:
                ok = modem.send(fp)
            return (True, f"已发送 {os.path.getsize(target)} 字节") if ok else (False, "发送被对端取消")

        if proto == "y":
            from ymodem import ModemSocket

            sock = ModemSocket(getc, putc)
            ok = sock.send([target])
            return (True, f"已发送 {os.path.getsize(target)} 字节") if ok else (False, "发送被对端取消")

        if proto == "z":
            return self._zmodem_send(target, getc, putc)

        return False, f"未知协议 {proto}"

    def _transfer_recv(self, proto, target, getc, putc):
        """设备从上位机接收文件（设备是 RECEIVER）。"""
        if proto == "x":
            import xmodem

            modem = xmodem.XMODEM(getc, putc)
            with open(target, "wb") as fp:
                ok = modem.recv(fp)
            return (True, f"已接收 {os.path.getsize(target)} 字节") if ok else (False, "接收被取消")

        if proto == "y":
            from ymodem import ModemSocket

            sock = ModemSocket(getc, putc)
            folder = os.path.dirname(os.path.abspath(target)) or "."
            ok = sock.recv(folder)
            return (True, f"已接收至 {folder}") if ok else (False, "接收被取消")

        if proto == "z":
            return self._zmodem_recv(target, getc, putc)

        return False, f"未知协议 {proto}"

    # ── ZModem 最小实现 ─────────────────────────────────────
    # 委托给模块级 ZModem 标准实现（见上文 ZModem 类）。
    def _zmodem_send(self, target, getc, putc):
        return ZModem(getc, putc, log=self._log).send(target)

    def _zmodem_recv(self, target, getc, putc):
        return ZModem(getc, putc, log=self._log).receive(target)


# ─────────────────────────────────────────────────────────────────────────────
# 参数解析 / 入口
# ─────────────────────────────────────────────────────────────────────────────

def parse_args(argv):
    p = argparse.ArgumentParser(
        description="TauTerm 串口会话测试假服务器（模拟 RT-Thread 设备）",
        epilog="示例: python scripts/test-serial-session.py --setup --near COM200\n"
               "      在 TauTerm 中连接远端端口 COM201",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("--near", default=NEAR_PORT, help=f"本脚本连接的近端端口（默认 {NEAR_PORT}）")
    p.add_argument("--far", default=FAR_PORT, help=f"TauTerm 连接的远端端口（默认 {FAR_PORT}）")
    p.add_argument("--baud", type=int, default=DEFAULT_BAUD, help="波特率（默认 115200）")
    p.add_argument("--setup", action="store_true", help="启动前先创建 com0com 端口对（需管理员）")
    p.add_argument("--teardown-port", metavar="PORT", help="删除指定端点所在端口对并退出")
    p.add_argument("--teardown-all", action="store_true",
                   help=f"删除预留段 ({RESERVED_BUS_BASE}-{RESERVED_BUS_END}) 内的 com0com "
                        "端口对并退出（不影响产品端口对）")
    p.add_argument("--dry-run", action="store_true", help="仅打印将执行的 setupc 命令，不实际执行")
    p.add_argument("--hex", action="store_true", help="以 HEX 视图打印所有收/发字节")
    p.add_argument("--respond", metavar="FILE", help="自动应答规则文件（模拟 Lua/脚本驱动）")
    p.add_argument("--no-banner", action="store_true", help="不输出启动横幅（适合重复连接测试）")
    p.add_argument("--log", metavar="FILE", help="将会话日志追加写入到文件")
    return p.parse_args(argv)


def main(argv=None):
    args = parse_args(argv if argv is not None else sys.argv[1:])

    if args.dry_run:
        com_dump_plan(args.near, args.far)
        return 0

    if args.teardown_all:
        if not is_admin():
            _ensure_admin_hint("remove all")
            return 1
        com_teardown_all()
        return 0

    if args.teardown_port:
        if not is_admin():
            _ensure_admin_hint("remove")
            return 1
        ok = com_remove_port(args.teardown_port)
        return 0 if ok else 1

    # 创建端口对
    app_created_pair = False
    if args.setup:
        if not os.path.isfile(SETUPC):
            print(f"[错误] 未找到 com0com 配套文件目录: {COM0COM_DIR}")
            return 1
        if not com_repair():
            print("[错误] com0com 环境修复失败，无法继续。")
            return 1
        bus, err = com_create_pair(args.near, args.far)
        if err:
            print(f"[错误] 创建端口对失败: {err}")
            return 1
        print(f"[信息] 已创建 com0com 端口对 (bus {bus}): {args.near} <-> {args.far}")
        app_created_pair = True

    # 打开近端端口（PnP 注册端口名可能有短暂延迟，失败时重试几次）
    device = None
    last_err = None
    for _ in range(5):
        try:
            device = RTTDevice(
                args.near, args.baud,
                hex_dump=args.hex,
                rules_path=args.respond,
                quiet_banner=args.no_banner,
                log_path=args.log,
            )
            break
        except serial.SerialException as e:
            last_err = e
            time.sleep(0.5)
    if device is None:
        print(f"[错误] 无法打开端口 {args.near}: {last_err}")
        print("  请确认：\n"
              "  1) 端口对已创建（可先 --setup 或 --dry-run 查看命令）\n"
              "  2) TauTerm 未占用该近端端口\n"
              f"  3) 远端端口应为 {args.far}，请检查 TauTerm 串口会话配置")
        if app_created_pair:
            com_remove_port(args.near)
        return 1

    try:
        device.run()
    except KeyboardInterrupt:
        print("\n[信息] 收到中断，正在退出...")
    finally:
        device.cleanup()
        if app_created_pair:
            print(f"[信息] 清理 com0com 端口对: {args.near} <-> {args.far}")
            com_remove_port(args.near)
    return 0


if __name__ == "__main__":
    sys.exit(main())
