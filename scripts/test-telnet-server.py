#!/usr/bin/env python3
"""
TauTerm Telnet 集成测试假服务器

模拟真实 Linux 终端（登录 → Shell 状态机），用于手动验证 TauTerm 的 Telnet 会话：
1. 连接后主动协商 WILL ECHO（服务器回显）→ 期望客户端应答 DO ECHO
2. login: → 输入任意用户名 → Password:（输入不回显）→ 欢迎横幅 + Shell 提示符
3. Shell 中执行模拟命令（ps / ls / ifconfig / uname 等）
4. 输入 "echo off" 后发送 IAC WONT ECHO → 期望客户端本地回显生效
5. 输入 "echo on" 后发送 IAC WILL ECHO → 期望本地回显关闭
6. 终端尺寸变化时输出收到的 NAWS 字节

用法: python scripts/test-telnet-server.py [端口，默认 23]
在 TauTerm 中新建 Telnet 会话连接 127.0.0.1:23（用户名/密码任意非空）。

注意: Windows 上 0-1023 为特权端口，监听 23 需以管理员身份运行；
普通权限可改用: python scripts/test-telnet-server.py 2323
"""
import asyncio
import sys

# 日志实时输出（管道/重定向下 print 默认块缓冲，会导致日志延迟或丢失）
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(line_buffering=True)

IAC = b"\xff"
WILL, WONT, DO, DONT = b"\xfb", b"\xfc", b"\xfd", b"\xfe"
SB, SE = b"\xfa", b"\xf0"
ECHO, SGA, NAWS, BINARY = b"\x01", b"\x03", b"\x1f", b"\x00"

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 23

# 会话阶段
PHASE_LOGIN, PHASE_PASSWORD, PHASE_SHELL = 0, 1, 2

# 服务器回显状态（初始假设回显；echo off/on 命令切换）
server_echo = True

# ── 模拟命令输出（静态文本） ──────────────────────────

PS_OUTPUT = (
    "PID   USER     TIME  COMMAND\r\n"
    "  1   root     0:01  /sbin/init\r\n"
    "  2   root     0:00  [kthreadd]\r\n"
    "  8   root     0:00  [kworker/0:0]\r\n"
    " 23   root     0:00  sshd: /usr/sbin/sshd\r\n"
    " 42   root     0:00  -bash\r\n"
).encode("utf-8")

IFCONFIG_OUTPUT = (
    "lo        Link encap:Local Loopback\r\n"
    "          inet addr:127.0.0.1  Mask:255.0.0.0\r\n"
    "          inet6 addr: ::1/128 Scope:Host\r\n"
    "          UP LOOPBACK RUNNING  MTU:65536  Metric:1\r\n"
    "\r\n"
    "eth0      Link encap:Ethernet  HWaddr 02:42:ac:11:00:02\r\n"
    "          inet addr:192.168.1.10  Bcast:192.168.1.255  Mask:255.255.255.0\r\n"
    "          inet6 addr: fe80::42:acff:fe11:2/64 Scope:Link\r\n"
    "          UP BROADCAST RUNNING MULTICAST  MTU:1500  Metric:1\r\n"
).encode("utf-8")

IP_ADDR_OUTPUT = (
    "1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN\r\n"
    "    link/loopback 00:00:00:00:00:00 brd 00:00:00:00:00:00\r\n"
    "    inet 127.0.0.1/8 scope host lo\r\n"
    "    inet6 ::1/128 scope host\r\n"
    "2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc fq_codel state UP\r\n"
    "    link/ether 02:42:ac:11:00:02 brd ff:ff:ff:ff:ff:ff\r\n"
    "    inet 192.168.1.10/24 brd 192.168.1.255 scope global eth0\r\n"
    "    inet6 fe80::42:acff:fe11:2/64 scope link\r\n"
).encode("utf-8")

LS_OUTPUT = (
    "build/  configs/  docs/  initrd.img  logs/  src/  tauterm.conf  tools/  usr/  var/\r\n"
).encode("utf-8")

LS_L_OUTPUT = (
    "drwxr-xr-x 2 root root  4096 Aug  8 09:12 build\r\n"
    "drwxr-xr-x 2 root root  4096 Aug  8 09:12 configs\r\n"
    "drwxr-xr-x 3 root root  4096 Aug  8 09:14 docs\r\n"
    "-rw-r--r-- 1 root root  512K Aug  8 10:01 initrd.img\r\n"
    "drwxr-xr-x 2 root root  4096 Aug  8 09:13 logs\r\n"
    "-rw-r--r-- 1 root root  2048 Aug  8 09:15 tauterm.conf\r\n"
    "-rw-r--r-- 1 root root  1024 Aug  8 09:15 usr\r\n"
    "drwxr-xr-x 2 root root  4096 Aug  8 09:12 var\r\n"
).encode("utf-8")

UNAME_OUTPUT = (
    "Linux mock-telnet 5.15.0-91-generic #101-Ubuntu SMP x86_64 GNU/Linux\r\n"
).encode("utf-8")

ISSUE_OUTPUT = (
    "TauTerm Mock OS 1.0 \\n \\l\r\n"
).encode("utf-8")

HELP_OUTPUT = (
    "可用命令（模拟输出）:\r\n"
    "  ps              查看进程列表\r\n"
    "  ifconfig        查看网络接口 (ip addr 亦同)\r\n"
    "  ls | ls -l      查看目录列表\r\n"
    "  uname -a        查看系统信息\r\n"
    "  cat /etc/issue  查看发行版信息\r\n"
    "  pwd / whoami    显示当前目录 / 用户\r\n"
    "\r\n"
    "Telnet 协商测试指令:\r\n"
    "  echo off        服务器发送 IAC WONT ECHO（验证 TauTerm 本地回显）\r\n"
    "  echo on         服务器发送 IAC WILL ECHO（恢复服务器回显）\r\n"
    "\r\n"
    "  bye / exit / logout  断开连接\r\n"
).encode("utf-8")

PWD_OUTPUT = "/root\r\n".encode("utf-8")


def shell_output(username: str, cmd: str) -> bytes:
    """命令 → 模拟输出字节"""
    c = cmd.strip()
    if c == "ps":
        return PS_OUTPUT
    if c in ("ifconfig", "ip addr"):
        return IFCONFIG_OUTPUT if c == "ifconfig" else IP_ADDR_OUTPUT
    if c == "ls":
        return LS_OUTPUT
    if c == "ls -l":
        return LS_L_OUTPUT
    if c == "uname -a":
        return UNAME_OUTPUT
    if c == "cat /etc/issue":
        return ISSUE_OUTPUT
    if c == "help":
        return HELP_OUTPUT
    if c == "pwd":
        return PWD_OUTPUT
    if c == "whoami":
        return (username + "\r\n").encode("utf-8")
    if c == "":
        return b""
    # 未知命令
    first = c.split()[0]
    return f"bash: {first}: command not found\r\n".encode("utf-8")


def prompt_for(username: str) -> bytes:
    """Shell 提示符：root 用 #，其他用户用 $"""
    suffix = "#" if username == "root" else "$"
    return f"{username}@mock:~{suffix} ".encode("utf-8")


async def handle(reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
    global server_echo
    peer = writer.get_extra_info("peername")
    print(f"[+] {peer} 已连接")

    phase = PHASE_LOGIN
    username = ""
    line_buf = b""
    # 解析缓冲：TCP 是字节流会任意分片，IAC/SB 序列可能跨包，
    # 未完整消费的字节保留在 proc_buf 中等待下一包（流式状态机）
    proc_buf = b""
    exit_session = False

    # 初始协商：WILL ECHO（服务器回显）+ DO SGA
    writer.write(IAC + WILL + ECHO + IAC + DO + SGA)
    await writer.drain()
    print("[协商] 发送 WILL ECHO, DO SGA")

    writer.write("\r\nTauTerm Mock Server 1.0\r\n".encode("utf-8"))
    writer.write(b"login: ")
    await writer.drain()

    try:
        while True:
            data = await reader.read(4096)
            if not data or exit_session:
                if exit_session:
                    print(f"[-] {peer} 退出")
                else:
                    print(f"[-] {peer} 断开")
                break

            proc_buf += data
            i = 0
            while i < len(proc_buf):
                if proc_buf[i] == 0xFF:
                    # IAC 命令：需至少 3 字节（IAC + 命令 + 选项）；不完整则保留等下一包
                    if i + 2 >= len(proc_buf):
                        break
                    cmd = proc_buf[i + 1]

                    if cmd == 0xFA:  # SB 子协商：找 IAC SE 结束，未结束保留跨包
                        se_idx = proc_buf.find(b"\xff\xf0", i + 2)
                        if se_idx < 0:
                            break
                        sub = proc_buf[i + 2:se_idx]
                        if sub and sub[0] == NAWS[0] and len(sub) >= 5:
                            w = sub[1] << 8 | sub[2]
                            h = sub[3] << 8 | sub[4]
                            print(f"[NAWS] 终端尺寸: {w} x {h}")
                        i = se_idx + 2
                        continue
                    if cmd == 0xFF:
                        # IAC IAC → 数据字节 0xFF（BINARY 模式），进行缓冲但不回显（避免乱码）
                        line_buf += b"\xff"
                        i += 2
                        continue
                    if cmd in (WILL[0], WONT[0], DO[0], DONT[0]):
                        opt = proc_buf[i + 2]
                        if cmd == DO[0]:
                            print(f"[协商] 客户端 DO {opt:#04x}")
                        elif cmd == DONT[0]:
                            print(f"[协商] 客户端 DONT {opt:#04x}")
                        elif cmd == WILL[0]:
                            print(f"[协商] 客户端 WILL {opt:#04x}")
                        else:
                            print(f"[协商] 客户端 WONT {opt:#04x}")
                        i += 3
                        continue
                    # 其他 IAC 命令（NOP/SE 等）：吞 2 字节
                    i += 2
                    continue

                ch = proc_buf[i]
                # 字符回显：服务器回显开启且非密码阶段（密码永不回显，真实终端行为）
                if server_echo and phase != PHASE_PASSWORD:
                    writer.write(bytes([ch]))

                # 行结束（真实终端以 \r 或 \n 结尾）
                if ch in (0x0D, 0x0A):
                    line = line_buf.strip(b"\r\n")
                    line_buf = b""

                    if phase == PHASE_LOGIN:
                        if line:
                            username = line.decode(errors="replace")
                            print(f"[登录] 用户名: {username}")
                            writer.write(b"\r\nPassword: ")
                            phase = PHASE_PASSWORD
                        else:
                            writer.write(b"\r\nlogin: ")

                    elif phase == PHASE_PASSWORD:
                        if line:
                            print(f"[登录] 用户 {username} 密码已输入（不回显）")
                            writer.write(b"\r\n")
                            writer.write("\r\nWelcome to TauTerm Mock Server\r\n"
                                         "Type 'help' for available commands.\r\n".encode("utf-8"))
                            writer.write(prompt_for(username))
                            phase = PHASE_SHELL
                        else:
                            # 密码不能为空，重新提示
                            writer.write(b"\r\nPassword: ")

                    else:  # PHASE_SHELL
                        cmd_text = line.decode(errors="replace")
                        if cmd_text == "echo off":
                            server_echo = False
                            writer.write(IAC + WONT + ECHO)
                            writer.write("\r\n[服务器回显已关闭 — 客户端应本地回显]\r\n".encode("utf-8"))
                            print("[协商] 发送 WONT ECHO → 客户端应本地回显")
                        elif cmd_text == "echo on":
                            server_echo = True
                            writer.write(IAC + WILL + ECHO)
                            writer.write("\r\n[服务器回显已恢复]\r\n".encode("utf-8"))
                            print("[协商] 发送 WILL ECHO → 客户端应停止本地回显")
                        elif cmd_text in ("bye", "exit", "logout"):
                            writer.write("\r\n再见！\r\n".encode("utf-8"))
                            await writer.drain()
                            exit_session = True
                            break
                        else:
                            out = shell_output(username, cmd_text)
                            if out:
                                writer.write(b"\r\n" + out)
                            else:
                                writer.write(b"\r\n")
                            print(f"[输入] {cmd_text}" if cmd_text else "[输入] (空行)")
                        writer.write(prompt_for(username))

                else:
                    line_buf += bytes([ch])

                i += 1

            # 保留未消费的字节（不完整的 IAC/SB 序列，等下一包续接）
            proc_buf = proc_buf[i:]
            await writer.drain()
    except (ConnectionResetError, BrokenPipeError):
        print(f"[-] {peer} 连接异常关闭")
    finally:
        writer.close()


async def main():
    try:
        server = await asyncio.start_server(handle, "127.0.0.1", PORT)
    except PermissionError as e:
        # Windows 上 0-1023 为特权端口，普通权限绑定会失败（WinError 10013）
        print(f"[错误] 绑定端口 {PORT} 权限不足: {e}")
        if PORT < 1024:
            print("  Windows 特权端口需以管理员身份运行本脚本，")
            print("  或改用非特权端口: python scripts/test-telnet-server.py 2323")
        sys.exit(1)
    except OSError as e:
        if hasattr(e, "winerror") and e.winerror == 10013:
            print(f"[错误] 绑定端口 {PORT} 权限不足（WinError 10013）")
            if PORT < 1024:
                print("  Windows 特权端口需以管理员身份运行本脚本，")
                print("  或改用非特权端口: python scripts/test-telnet-server.py 2323")
            sys.exit(1)
        if e.errno == 98 or (hasattr(e, "winerror") and e.winerror == 10048):
            print(f"[错误] 端口 {PORT} 已被占用（可能是系统 Telnet 服务）")
            print("  改用: python scripts/test-telnet-server.py 2323")
            sys.exit(1)
        raise
    print(f"[*] 假 Telnet 服务器监听 127.0.0.1:{PORT}")
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    asyncio.run(main())
