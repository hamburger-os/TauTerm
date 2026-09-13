#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
TauTerm TRDP 会话集成测试入口

本脚本严格围绕 TauTerm 当前 TRDP 边界设计：
  * 标准互通：调用仓库 tools/trdp-test-peer 中基于 vendored TCNOpen 3.0.0.0
    构建的 reference peer。它与 TauTerm native bridge 是两个独立 application session，
    用于验证真正的 TCNOpen ↔ TauTerm PD/MD 互操作。
  * Decoder / Monitor 鲁棒性：Python 仅构造 40-byte TRDP PD wire header，用于发送
    valid / bad CRC / bad version / bad length / truncated / duplicate / out-of-order /
    sequence-gap 等故障帧。CRC 与字段布局与 TauTerm 当前 Rust capture decoder 一致。

为什么不在 Python 中重新实现完整 MD：TauTerm 的工程基线是仓库 vendored TCNOpen
3.0.0.0；标准 MD 的 request/reply/query/confirm/TCP 互通直接调用 reference peer，避免
维护第二套可能漂移的 TRDP 状态机。Python raw 模式只承担故障注入和 Monitor 验证。

推荐完整人工回归矩阵：

  0. 首次运行先检查环境（若 reference peer 不存在会自动调用仓库 bootstrap）：
     python scripts/test-trdp-session.py doctor

  1. TauTerm PD Subscriber <- TCNOpen Publisher
     python scripts/test-trdp-session.py peer pd-publisher \
       --own-ip 10.10.0.20 --peer-ip 239.255.1.1 --comid 2001

  2. TauTerm PD Publisher -> TCNOpen Subscriber
     python scripts/test-trdp-session.py peer pd-subscriber \
       --own-ip 10.10.0.20 --peer-ip 239.255.1.1 --comid 2001

  3. TauTerm PD Request (Pr) -> TCNOpen Pull Provider -> Pp
     python scripts/test-trdp-session.py peer pd-pull-provider \
       --own-ip 10.10.0.20 --peer-ip 239.255.2.2 --comid 2002
     TauTerm request destination 指向 10.10.0.20，Reply ComID=2002，Reply IP=239.255.2.2。

  4. TauTerm MD Request -> reference peer Reply（UDP / TCP）
     python scripts/test-trdp-session.py peer md-replier --own-ip 10.10.0.20 --comid 4001
     python scripts/test-trdp-session.py peer md-replier-tcp --own-ip 10.10.0.20 --comid 4001

  5. TauTerm MD Request -> ReplyQuery -> TauTerm Confirm
     python scripts/test-trdp-session.py peer md-replier-query --own-ip 10.10.0.20 --comid 4001
     TCP 同理使用 md-replier-query-tcp。

  6. reference peer MD Request -> TauTerm Listener/Replier
     python scripts/test-trdp-session.py peer md-requester \
       --own-ip 10.10.0.20 --peer-ip 10.10.0.10 --comid 4001
     TCP 使用 md-requester-tcp。

  7. TauTerm Monitor / Analysis 故障与流统计
     python scripts/test-trdp-session.py raw-pd --source-ip 10.10.0.20 \
       --dest-ip 239.255.1.1 --comid 9001 --scenario mixed
     Monitor 应能区分 CRC/protocol validity，序号跳变应反映 missed；畸形帧不能导致崩溃。

  8. 纯 Python 观察 TauTerm PD Publisher（辅助人工检查，不替代 TCNOpen interop）
     python scripts/test-trdp-session.py listen-pd --bind-ip 0.0.0.0 \
       --group 239.255.1.1 --expected-comid 2001

  9. Link A / B
     在两块实际接口或隔离网络上分别启动两个 peer 实例；TauTerm 对象选择 Both 后，
     两侧都应收到 Pd。不要把 A/B 自动理解为主备；它们在 TauTerm 中是网络路径。

默认端口遵循当前 vendored TCNOpen：PD UDP 17224，MD UDP/TCP 17225。
最终 Windows ↔ Linux/设备互通请使用网卡真实 IPv4，不要用 0.0.0.0 作为发送端 own IP。
防火墙需允许对应端口和多播。

常用故障场景：
  valid         连续合法 Pd
  bad-crc       Header FCS 错误
  bad-version   protocolVersion=0x0200
  bad-length    datasetLength 大于实际 payload
  truncated     声明长度正确但实际 payload 截断
  wrong-comid   发送到相同链路但 ComID+1
  duplicate     重复 sequence counter
  seq-gap       人为跳过序号，验证 missed 统计
  out-of-order  发送 N, N+2, N+1
  mixed         依次发送上述代表场景

--self-test 不需要网络、TCNOpen 或 TauTerm，只校验 Python PD header/CRC/解析逻辑。
"""
from __future__ import annotations

import argparse
import ipaddress
import os
import platform
import shutil
import socket
import struct
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(line_buffering=True)

PD_PORT = 17224
MD_PORT = 17225
PD_HEADER_SIZE = 40
TRDP_PROTOCOL_VERSION = 0x0100
MAX_PD_DATA = 1432
PD_TYPES = {"Pd": 0x5064, "Pp": 0x5070, "Pr": 0x5072, "Pe": 0x5065}
PEER_MODES = (
    "pd-publisher",
    "pd-pull-provider",
    "pd-subscriber",
    "md-requester",
    "md-requester-tcp",
    "md-replier",
    "md-replier-query",
    "md-replier-tcp",
    "md-replier-query-tcp",
)


def repo_root() -> Path:
    here = Path(__file__).resolve()
    if here.parent.name == "scripts":
        return here.parent.parent
    cwd = Path.cwd()
    return cwd if (cwd / "scripts").is_dir() else here.parent


def peer_path(root: Path) -> Path:
    suffix = ".exe" if os.name == "nt" else ""
    return root / "tools" / "trdp-test-peer" / "bin" / f"trdp-test-peer{suffix}"


def bootstrap_command(root: Path) -> list[str]:
    if os.name == "nt":
        powershell = shutil.which("pwsh") or shutil.which("powershell")
        if not powershell:
            raise RuntimeError("未找到 PowerShell，无法自动构建 TRDP reference peer")
        return [powershell, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(root / "scripts" / "bootstrap-trdp.ps1")]
    bash = shutil.which("bash")
    if not bash:
        raise RuntimeError("未找到 bash，无法自动构建 TRDP reference peer")
    return [bash, str(root / "scripts" / "bootstrap-trdp.sh")]


def ensure_peer(*, build: bool = True) -> Path:
    root = repo_root()
    peer = peer_path(root)
    if peer.is_file():
        return peer
    if not build:
        raise FileNotFoundError(f"TRDP reference peer 不存在: {peer}")
    script = root / "scripts" / ("bootstrap-trdp.ps1" if os.name == "nt" else "bootstrap-trdp.sh")
    if not script.is_file():
        raise FileNotFoundError(f"找不到 TRDP bootstrap: {script}")
    print(f"[准备] reference peer 不存在，执行仓库 bootstrap: {script}")
    result = subprocess.run(bootstrap_command(root), cwd=root)
    if result.returncode != 0 or not peer.is_file():
        raise RuntimeError(f"TRDP bootstrap 失败（exit={result.returncode}），未生成 {peer}")
    return peer


def trdp_crc32(data: bytes) -> int:
    """IEEE CRC-32 reflected form used by TauTerm's current capture decoder."""
    crc = 0xFFFFFFFF
    for byte in data:
        crc ^= byte
        for _ in range(8):
            mask = -(crc & 1) & 0xFFFFFFFF
            crc = ((crc >> 1) ^ (0xEDB88320 & mask)) & 0xFFFFFFFF
    return (~crc) & 0xFFFFFFFF


def ipv4_u32(address: str) -> int:
    return int(ipaddress.IPv4Address(address))


def bytes_from_hex(text: str) -> bytes:
    cleaned = text.replace(" ", "").replace(":", "").replace("-", "")
    if len(cleaned) % 2:
        raise argparse.ArgumentTypeError("payload hex 必须包含完整字节")
    try:
        data = bytes.fromhex(cleaned)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(str(exc)) from exc
    if len(data) > MAX_PD_DATA:
        raise argparse.ArgumentTypeError(f"PD payload 最大 {MAX_PD_DATA} bytes")
    return data


def pretty_hex(data: bytes, limit: int = 64) -> str:
    shown = data[:limit]
    text = " ".join(f"{byte:02X}" for byte in shown)
    return text + (" ..." if len(data) > limit else "")


@dataclass(frozen=True)
class PdPacket:
    sequence: int
    protocol_version: int
    msg_type: str
    com_id: int
    etb_topo: int
    op_topo: int
    declared_length: int
    service_id: int
    reply_com_id: int
    reply_ip: str
    stored_fcs: int
    crc_valid: bool
    protocol_valid: bool
    payload: bytes
    complete: bool


def build_pd_packet(
    *,
    sequence: int,
    com_id: int,
    payload: bytes,
    msg_type: str = "Pd",
    protocol_version: int = TRDP_PROTOCOL_VERSION,
    etb_topo: int = 0,
    op_topo: int = 0,
    service_id: int = 0,
    reply_com_id: int = 0,
    reply_ip: str = "0.0.0.0",
    declared_length: Optional[int] = None,
    bad_crc: bool = False,
) -> bytes:
    if msg_type not in PD_TYPES:
        raise ValueError(f"unsupported PD msg type: {msg_type}")
    if not 0 <= com_id <= 0xFFFFFFFF:
        raise ValueError("ComID must be 0..0xffffffff")
    if len(payload) > MAX_PD_DATA:
        raise ValueError(f"payload exceeds {MAX_PD_DATA} bytes")
    data_len = len(payload) if declared_length is None else declared_length
    if not 0 <= data_len <= 0xFFFFFFFF:
        raise ValueError("declared dataset length out of range")
    header_without_fcs = struct.pack(
        ">IHHIIIIIII",
        sequence & 0xFFFFFFFF,
        protocol_version & 0xFFFF,
        PD_TYPES[msg_type],
        com_id,
        etb_topo & 0xFFFFFFFF,
        op_topo & 0xFFFFFFFF,
        data_len,
        service_id & 0xFFFFFFFF,
        reply_com_id & 0xFFFFFFFF,
        ipv4_u32(reply_ip),
    )
    assert len(header_without_fcs) == 36
    fcs = trdp_crc32(header_without_fcs)
    if bad_crc:
        fcs ^= 0x00000001
    return header_without_fcs + struct.pack("<I", fcs) + payload


def parse_pd_packet(data: bytes) -> PdPacket:
    if len(data) < PD_HEADER_SIZE:
        raise ValueError(f"PD packet too short: {len(data)}")
    seq, version, msg, com_id, etb, op, length, service, reply_com, reply_ip = struct.unpack(">IHHIIIIIII", data[:36])
    msg_text = bytes([(msg >> 8) & 0xFF, msg & 0xFF]).decode("ascii", errors="replace")
    stored = struct.unpack("<I", data[36:40])[0]
    calculated = trdp_crc32(data[:36])
    end = PD_HEADER_SIZE + length
    complete = end <= len(data)
    payload = data[PD_HEADER_SIZE:min(end, len(data))]
    return PdPacket(
        sequence=seq,
        protocol_version=version,
        msg_type=msg_text,
        com_id=com_id,
        etb_topo=etb,
        op_topo=op,
        declared_length=length,
        service_id=service,
        reply_com_id=reply_com,
        reply_ip=str(ipaddress.IPv4Address(reply_ip)),
        stored_fcs=stored,
        crc_valid=stored == calculated,
        protocol_valid=(version & 0xFF00) == 0x0100 and complete,
        payload=payload,
        complete=complete,
    )


def configure_multicast_sender(sock: socket.socket, source_ip: str, destination: str, ttl: int) -> None:
    sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, struct.pack("B", ttl))
    sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_LOOP, 1)
    if source_ip != "0.0.0.0":
        sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(source_ip))


def make_udp_sender(source_ip: str, destination: str, source_port: int, ttl: int) -> socket.socket:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    if source_ip != "0.0.0.0" or source_port:
        sock.bind((source_ip, source_port))
    if ipaddress.ip_address(destination).is_multicast:
        configure_multicast_sender(sock, source_ip, destination, ttl)
    return sock


def scenario_packets(args: argparse.Namespace) -> Iterable[tuple[str, bytes, int]]:
    base = args.sequence
    payload = args.payload

    def packet(label: str, seq: int, **overrides) -> tuple[str, bytes, int]:
        params = dict(
            sequence=seq,
            com_id=args.comid,
            payload=payload,
            msg_type=args.msg_type,
            etb_topo=args.etb_topo,
            op_topo=args.op_topo,
            service_id=args.service_id,
            reply_com_id=args.reply_comid,
            reply_ip=args.reply_ip,
        )
        params.update(overrides)
        return label, build_pd_packet(**params), seq

    scenario = args.scenario
    if scenario == "valid":
        for index in range(args.count):
            yield packet("valid", base + index)
    elif scenario == "bad-crc":
        yield packet("bad-crc", base, bad_crc=True)
    elif scenario == "bad-version":
        yield packet("bad-version", base, protocol_version=0x0200)
    elif scenario == "bad-length":
        yield packet("bad-length", base, declared_length=len(payload) + max(1, args.length_delta))
    elif scenario == "truncated":
        full = packet("truncated", base)[1]
        remove = min(max(1, args.truncate_bytes), max(1, len(payload)))
        yield "truncated", full[:-remove], base
    elif scenario == "wrong-comid":
        yield packet("wrong-comid", base, com_id=(args.comid + 1) & 0xFFFFFFFF)
    elif scenario == "duplicate":
        yield packet("duplicate-1", base)
        yield packet("duplicate-2", base)
    elif scenario == "seq-gap":
        yield packet("seq-before-gap", base)
        yield packet("seq-after-gap", base + max(2, args.gap))
    elif scenario == "out-of-order":
        yield packet("seq-N", base)
        yield packet("seq-N+2", base + 2)
        yield packet("seq-N+1", base + 1)
    elif scenario == "mixed":
        yield packet("valid", base)
        yield packet("bad-crc", base + 1, bad_crc=True)
        yield packet("bad-version", base + 2, protocol_version=0x0200)
        yield packet("bad-length", base + 3, declared_length=len(payload) + max(1, args.length_delta))
        full = packet("truncated", base + 4)[1]
        remove = min(max(1, args.truncate_bytes), max(1, len(payload)))
        yield "truncated", full[:-remove], base + 4
        yield packet("wrong-comid", base + 5, com_id=(args.comid + 1) & 0xFFFFFFFF)
        yield packet("gap-before", base + 6)
        yield packet("gap-after", base + 9)
        yield packet("duplicate-1", base + 10)
        yield packet("duplicate-2", base + 10)
        yield packet("out-order-high", base + 12)
        yield packet("out-order-low", base + 11)
    else:
        raise ValueError(f"unknown scenario {scenario}")


def run_raw_pd(args: argparse.Namespace) -> int:
    with make_udp_sender(args.source_ip, args.dest_ip, args.source_port, args.ttl) as sock:
        print(f"[开始] raw PD scenario={args.scenario} src={args.source_ip or '0.0.0.0'} dst={args.dest_ip}:{args.port} ComID={args.comid}")
        sent = 0
        for label, packet, seq in scenario_packets(args):
            sock.sendto(packet, (args.dest_ip, args.port))
            parsed = None
            try:
                parsed = parse_pd_packet(packet)
            except ValueError:
                pass
            state = f"crc={'OK' if parsed and parsed.crc_valid else 'BAD'} protocol={'OK' if parsed and parsed.protocol_valid else 'BAD'}" if parsed else "truncated-header"
            print(f"[TX] {label:<16} seq={seq:<10} bytes={len(packet):<4} {state}")
            sent += 1
            if args.interval_ms:
                time.sleep(args.interval_ms / 1000.0)
        print(f"[结束] sent={sent}")
    return 0


def join_group(sock: socket.socket, group: str, interface_ip: str) -> None:
    sock.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, socket.inet_aton(group) + socket.inet_aton(interface_ip))


def run_listen_pd(args: argparse.Namespace) -> int:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        sock.bind((args.bind_ip, args.port))
        if args.group:
            if not ipaddress.ip_address(args.group).is_multicast:
                raise SystemExit("--group 必须为 IPv4 multicast 地址")
            join_group(sock, args.group, args.interface_ip)
            print(f"[加入] multicast {args.group} via {args.interface_ip}")
        sock.settimeout(0.5)
        print(f"[监听] {args.bind_ip}:{args.port}，Ctrl-C 退出")
        start = time.monotonic()
        count = valid = errors = 0
        last_seq: dict[tuple[str, int], int] = {}
        while True:
            if args.duration > 0 and time.monotonic() - start >= args.duration:
                break
            try:
                data, peer = sock.recvfrom(65535)
            except socket.timeout:
                continue
            except KeyboardInterrupt:
                break
            count += 1
            try:
                packet = parse_pd_packet(data)
            except ValueError as exc:
                errors += 1
                print(f"[RX] {peer[0]} bytes={len(data)} parse-error={exc}")
                continue
            if args.expected_comid is not None and packet.com_id != args.expected_comid:
                print(f"[RX] 忽略 ComID={packet.com_id} expected={args.expected_comid}")
                continue
            key = (peer[0], packet.com_id)
            previous = last_seq.get(key)
            delta = None if previous is None else (packet.sequence - previous) & 0xFFFFFFFF
            last_seq[key] = packet.sequence
            is_valid = packet.crc_valid and packet.protocol_valid
            valid += int(is_valid)
            errors += int(not is_valid)
            seq_note = "" if delta is None else f" delta={delta}"
            print(f"[RX] {peer[0]} {packet.msg_type} ComID={packet.com_id} seq={packet.sequence}{seq_note} len={packet.declared_length} crc={'OK' if packet.crc_valid else 'BAD'} protocol={'OK' if packet.protocol_valid else 'BAD'} payload={pretty_hex(packet.payload)}")
        print(f"[结果] packets={count} valid={valid} errors={errors}")
        return 1 if args.fail_on_error and errors else 0
    finally:
        sock.close()


def run_peer(args: argparse.Namespace) -> int:
    try:
        peer = ensure_peer(build=not args.no_build)
    except Exception as exc:
        print(f"[错误] {exc}", file=sys.stderr)
        return 2
    peer_ip = args.peer_ip
    if peer_ip is None:
        peer_ip = "0.0.0.0" if args.mode.startswith("md-replier") else "127.0.0.1"
    command = [str(peer), args.mode, args.own_ip, peer_ip, str(args.comid)]
    if args.duration > 0:
        command.append(str(args.duration))
    print("[启动]", " ".join(command))
    env = os.environ.copy()
    if args.debug:
        env["TAUTERM_TRDP_DEBUG"] = "1"
    try:
        return subprocess.call(command, cwd=repo_root(), env=env)
    except KeyboardInterrupt:
        return 130


def run_doctor(args: argparse.Namespace) -> int:
    root = repo_root()
    print(f"Repository : {root}")
    print(f"Platform   : {platform.platform()}")
    print(f"Python     : {sys.version.split()[0]}")
    print(f"PD port    : {PD_PORT}")
    print(f"MD port    : {MD_PORT}")
    peer = peer_path(root)
    print(f"Peer       : {peer} ({'OK' if peer.is_file() else 'missing'})")
    if peer.is_file():
        result = subprocess.run([str(peer), "--help"], cwd=root, capture_output=True, text=True)
        print(f"Peer help  : exit={result.returncode}")
        return 0 if result.returncode == 0 else 1
    if args.no_build:
        return 1
    try:
        built = ensure_peer(build=True)
    except Exception as exc:
        print(f"[错误] {exc}", file=sys.stderr)
        return 2
    print(f"Peer built : {built}")
    return 0


def run_matrix() -> int:
    text = """
TauTerm TRDP 手工回归矩阵
==========================
1. PD Subscribe : peer pd-publisher      -> TauTerm PD Subscriber
2. PD Publish   : TauTerm PD Publisher  -> peer pd-subscriber
3. PD Pull      : TauTerm PD Request    -> peer pd-pull-provider -> Pp
4. MD UDP       : TauTerm Request       -> peer md-replier
5. MD Query     : TauTerm Request       -> peer md-replier-query -> Confirm
6. MD TCP       : TauTerm Request       -> peer md-replier-tcp / query-tcp
7. MD Reverse   : peer md-requester     -> TauTerm Listener/Replier
8. MD ReverseTCP: peer md-requester-tcp -> TauTerm Listener/Replier
9. Monitor      : raw-pd --scenario mixed；检查 CRC/protocol/missed/flow/jitter
10. A/B Links   : 两个真实接口分别运行 peer；TauTerm link=both 应双链路可见
11. Lifecycle   : 对象 Start/Stop、Session Disconnect/Reconnect 后重复 1..8
12. Stress      : PD 长时间周期发送；Monitor 检查 packet count / missed / jitter

更具体的命令示例请查看脚本顶部注释或各子命令 --help。
""".strip()
    print(text)
    return 0


def self_test() -> int:
    assert trdp_crc32(b"123456789") == 0xCBF43926
    payload = bytes.fromhex("01 02 03 04")
    raw = build_pd_packet(sequence=7, com_id=2001, payload=payload)
    assert len(raw) == 44
    parsed = parse_pd_packet(raw)
    assert parsed.sequence == 7
    assert parsed.protocol_version == 0x0100
    assert parsed.msg_type == "Pd"
    assert parsed.com_id == 2001
    assert parsed.declared_length == 4
    assert parsed.payload == payload
    assert parsed.crc_valid and parsed.protocol_valid and parsed.complete
    bad_crc = build_pd_packet(sequence=8, com_id=2001, payload=payload, bad_crc=True)
    assert not parse_pd_packet(bad_crc).crc_valid
    bad_version = build_pd_packet(sequence=9, com_id=2001, payload=payload, protocol_version=0x0200)
    assert not parse_pd_packet(bad_version).protocol_valid
    bad_length = build_pd_packet(sequence=10, com_id=2001, payload=payload, declared_length=8)
    parsed_bad_length = parse_pd_packet(bad_length)
    assert parsed_bad_length.crc_valid and not parsed_bad_length.protocol_valid and not parsed_bad_length.complete
    request = build_pd_packet(sequence=1, com_id=2002, payload=b"", msg_type="Pr", reply_com_id=2002, reply_ip="239.255.2.2")
    parsed_request = parse_pd_packet(request)
    assert parsed_request.msg_type == "Pr" and parsed_request.reply_com_id == 2002
    assert parsed_request.reply_ip == "239.255.2.2"
    print("TRDP test fixture self-test passed.")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="TauTerm TRDP TCNOpen 互通 + PD 故障注入测试入口",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="使用 `matrix` 查看完整人工回归矩阵；各子命令均支持 --help。",
    )
    parser.add_argument("--self-test", action="store_true", help="仅运行 Python PD codec/CRC 自检")
    sub = parser.add_subparsers(dest="command")
    doctor = sub.add_parser("doctor", help="检查/按需构建仓库 TCNOpen reference peer")
    doctor.add_argument("--no-build", action="store_true", help="缺少 peer 时只报告，不执行 bootstrap")
    sub.add_parser("matrix", help="打印推荐的完整人工回归矩阵")
    peer = sub.add_parser("peer", help="运行仓库 TCNOpen 3.0.0.0 reference peer")
    peer.add_argument("mode", choices=PEER_MODES)
    peer.add_argument("--own-ip", required=True, help="reference peer 所在真实 IPv4")
    peer.add_argument("--peer-ip", help="目标/组播 IPv4；MD replier 可省略，默认 0.0.0.0")
    peer.add_argument("--comid", type=int, required=True)
    peer.add_argument("--duration", type=int, default=0, help="运行秒数；0=直到 Ctrl-C")
    peer.add_argument("--debug", action="store_true", help="启用 TCNOpen debug log")
    peer.add_argument("--no-build", action="store_true", help="reference peer 缺失时不要自动 bootstrap")
    raw = sub.add_parser("raw-pd", help="发送合法或畸形 TRDP PD UDP 帧，重点测试 Monitor/decoder")
    raw.add_argument("--source-ip", default="0.0.0.0")
    raw.add_argument("--source-port", type=int, default=0)
    raw.add_argument("--dest-ip", required=True)
    raw.add_argument("--port", type=int, default=PD_PORT)
    raw.add_argument("--comid", type=int, default=9001)
    raw.add_argument("--msg-type", choices=tuple(PD_TYPES), default="Pd")
    raw.add_argument("--sequence", type=lambda v: int(v, 0), default=1)
    raw.add_argument("--payload", type=bytes_from_hex, default=bytes.fromhex("01 02 03 04"), help="十六进制 payload")
    raw.add_argument("--scenario", choices=("valid", "bad-crc", "bad-version", "bad-length", "truncated", "wrong-comid", "duplicate", "seq-gap", "out-of-order", "mixed"), default="valid")
    raw.add_argument("--count", type=int, default=20, help="valid 场景连续发送数量")
    raw.add_argument("--interval-ms", type=float, default=100.0)
    raw.add_argument("--gap", type=int, default=4, help="seq-gap 场景的序号跨度")
    raw.add_argument("--length-delta", type=int, default=4)
    raw.add_argument("--truncate-bytes", type=int, default=1)
    raw.add_argument("--ttl", type=int, default=64)
    raw.add_argument("--etb-topo", type=lambda v: int(v, 0), default=0)
    raw.add_argument("--op-topo", type=lambda v: int(v, 0), default=0)
    raw.add_argument("--service-id", type=lambda v: int(v, 0), default=0)
    raw.add_argument("--reply-comid", type=int, default=0)
    raw.add_argument("--reply-ip", default="0.0.0.0")
    listen = sub.add_parser("listen-pd", help="纯 Python PD 监听器，辅助观察 TauTerm Publisher")
    listen.add_argument("--bind-ip", default="0.0.0.0")
    listen.add_argument("--port", type=int, default=PD_PORT)
    listen.add_argument("--group", help="需要加入的 multicast group")
    listen.add_argument("--interface-ip", default="0.0.0.0", help="加入 multicast 的本机接口 IPv4")
    listen.add_argument("--expected-comid", type=int)
    listen.add_argument("--duration", type=float, default=0.0)
    listen.add_argument("--fail-on-error", action="store_true")
    return parser


def validate_ipv4(parser: argparse.ArgumentParser, value: str, name: str) -> None:
    try:
        ipaddress.IPv4Address(value)
    except ipaddress.AddressValueError:
        parser.error(f"{name} 不是合法 IPv4: {value}")


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if not args.command:
        parser.print_help()
        return 2
    if args.command == "doctor":
        return run_doctor(args)
    if args.command == "matrix":
        return run_matrix()
    if args.command == "peer":
        validate_ipv4(parser, args.own_ip, "--own-ip")
        if args.peer_ip is not None:
            validate_ipv4(parser, args.peer_ip, "--peer-ip")
        if not 1 <= args.comid <= 0xFFFFFFFF:
            parser.error("--comid 必须为 1..0xffffffff")
        return run_peer(args)
    if args.command == "raw-pd":
        validate_ipv4(parser, args.source_ip, "--source-ip")
        validate_ipv4(parser, args.dest_ip, "--dest-ip")
        validate_ipv4(parser, args.reply_ip, "--reply-ip")
        if not 0 <= args.comid <= 0xFFFFFFFF:
            parser.error("--comid 必须为 0..0xffffffff")
        if args.count < 1:
            parser.error("--count 必须 >= 1")
        return run_raw_pd(args)
    validate_ipv4(parser, args.bind_ip, "--bind-ip")
    validate_ipv4(parser, args.interface_ip, "--interface-ip")
    if args.group:
        validate_ipv4(parser, args.group, "--group")
    return run_listen_pd(args)


if __name__ == "__main__":
    raise SystemExit(main())
