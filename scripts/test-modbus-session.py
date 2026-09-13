#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
TauTerm Modbus 会话完整人工回归测试夹具。

用途：
  1) TauTerm Client -> 本脚本 Server：验证 RTU / ASCII / TCP、功能码、异常和故障恢复。
  2) 本脚本 Client -> TauTerm Server Simulator：验证 TauTerm 服务端协议行为。

快速开始：
  python scripts/test-modbus-session.py --self-test
  python scripts/test-modbus-session.py server tcp --host 127.0.0.1 --port 1502
  python scripts/test-modbus-session.py client tcp --host 127.0.0.1 --port 1502 --suite all

串口：RTU/ASCII 需要 pyserial。Windows 可与 test-serial-session.py 相同，使用
COM200 <-> COM201 虚拟串口对；脚本使用一端，TauTerm 使用另一端。

覆盖：FC01/02/03/04/05/06/0F/10/16/17，串行专属 FC07/08/0B/0C/11，
高级 FC14/15/18/2B-0E；CRC/LRC/MBAP/TID；广播写；非法地址/数量；TCP
分片、延时、无响应、强制异常、错误 TID、错误 MBAP Length、截断、主动断连。

故障示例：
  --delay-ms 800           延迟响应
  --drop-every 3           每 3 个请求无响应（请求仍执行）
  --force-exception 0x04   强制异常（不执行真实写）
  --bad-tid-every 2        错误 TID
  --bad-length-every 2     错误 MBAP Length
  --truncate-every 2       截断响应
  --close-every 2          响应前主动断连
  --fragment-size 3        TCP 每 3 字节分片发送

注意：TauTerm Server Simulator 只允许协议写修改已在工作台定义的地址。用本脚本
client 测 TauTerm Server 前，先定义 coils/discrete/holding/input 的 0..31 地址。
"""
from __future__ import annotations

import argparse
import binascii
import socket
import struct
import sys
import threading
import time
from dataclasses import dataclass, field
from typing import Callable, Optional

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(line_buffering=True)

SERIAL_ONLY = {0x07, 0x08, 0x0B, 0x0C, 0x11}
EX_ILLEGAL_FUNCTION = 0x01
EX_ILLEGAL_ADDRESS = 0x02
EX_ILLEGAL_VALUE = 0x03


def n(value: str) -> int:
    return int(value, 0)


def hx(data: bytes) -> str:
    return " ".join(f"{b:02X}" for b in data)


def u16(data: bytes, offset: int) -> int:
    if offset + 2 > len(data):
        raise ValueError("字段截断")
    return struct.unpack_from(">H", data, offset)[0]


def crc16(data: bytes) -> int:
    crc = 0xFFFF
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc & 0xFFFF


def lrc(data: bytes) -> int:
    return (-sum(data)) & 0xFF


def pack_bits(values: list[bool]) -> bytes:
    out = bytearray((len(values) + 7) // 8)
    for i, value in enumerate(values):
        if value:
            out[i // 8] |= 1 << (i % 8)
    return bytes(out)


def unpack_bits(data: bytes, count: int) -> list[bool]:
    return [bool(data[i // 8] & (1 << (i % 8))) for i in range(count)]


class ModbusError(Exception):
    def __init__(self, code: int):
        super().__init__(f"Modbus exception 0x{code:02X}")
        self.code = code


def span(address: int, quantity: int, maximum: int) -> None:
    if not 1 <= quantity <= maximum:
        raise ModbusError(EX_ILLEGAL_VALUE)
    if address + quantity - 1 > 0xFFFF:
        raise ModbusError(EX_ILLEGAL_ADDRESS)


@dataclass
class Model:
    defined: int = 256
    coils: dict[int, bool] = field(default_factory=dict)
    holding: dict[int, int] = field(default_factory=dict)
    files: dict[tuple[int, int], int] = field(default_factory=dict)
    fifo: dict[int, list[int]] = field(default_factory=lambda: {0: [0x1111, 0x2222, 0x3333]})
    exception_status: int = 0
    events: int = 0
    messages: int = 0

    def check(self, address: int, quantity: int = 1) -> None:
        if quantity < 1 or address < 0 or address + quantity > self.defined:
            raise ModbusError(EX_ILLEGAL_ADDRESS)

    def coil(self, address: int) -> bool:
        self.check(address)
        return self.coils.get(address, address % 3 == 0)

    def holding_reg(self, address: int) -> int:
        self.check(address)
        return self.holding.get(address, (0x1000 + 17 * address) & 0xFFFF)

    def execute(self, pdu: bytes, serial: bool) -> bytes:
        if not pdu:
            raise ModbusError(EX_ILLEGAL_FUNCTION)
        fc, data = pdu[0], pdu[1:]
        if not serial and fc in SERIAL_ONLY:
            raise ModbusError(EX_ILLEGAL_FUNCTION)
        clear = fc == 0x08 and len(data) >= 4 and u16(data, 0) == 0x000A
        try:
            response = self._execute(fc, data)
        finally:
            if not clear:
                self.messages = (self.messages + 1) & 0xFFFF
        if fc != 0x0B and not clear:
            self.events = (self.events + 1) & 0xFFFF
        return response

    def _execute(self, fc: int, data: bytes) -> bytes:
        if fc in (1, 2):
            if len(data) != 4:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, qty = u16(data, 0), u16(data, 2)
            span(address, qty, 2000)
            self.check(address, qty)
            values = [self.coil(address + i) if fc == 1 else (address + i) % 2 == 1 for i in range(qty)]
            packed = pack_bits(values)
            return bytes([fc, len(packed)]) + packed
        if fc in (3, 4):
            if len(data) != 4:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, qty = u16(data, 0), u16(data, 2)
            span(address, qty, 125)
            self.check(address, qty)
            values = [self.holding_reg(address + i) if fc == 3 else (0x2000 + 29 * (address + i)) & 0xFFFF for i in range(qty)]
            return bytes([fc, qty * 2]) + b"".join(struct.pack(">H", v) for v in values)
        if fc == 5:
            if len(data) != 4:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, raw = u16(data, 0), u16(data, 2)
            self.check(address)
            if raw not in (0, 0xFF00):
                raise ModbusError(EX_ILLEGAL_VALUE)
            self.coils[address] = raw == 0xFF00
            return bytes([fc]) + data
        if fc == 6:
            if len(data) != 4:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, value = u16(data, 0), u16(data, 2)
            self.check(address)
            self.holding[address] = value
            return bytes([fc]) + data
        if fc == 7:
            if data:
                raise ModbusError(EX_ILLEGAL_VALUE)
            return bytes([fc, self.exception_status])
        if fc == 8:
            if len(data) < 4 or len(data) % 2:
                raise ModbusError(EX_ILLEGAL_VALUE)
            sub = u16(data, 0)
            if sub == 0:
                return bytes([fc]) + data
            if sub == 0x000A and data[2:] == b"\x00\x00":
                self.events = self.messages = self.exception_status = 0
                return bytes([fc]) + data
            raise ModbusError(EX_ILLEGAL_VALUE)
        if fc == 0x0B:
            if data:
                raise ModbusError(EX_ILLEGAL_VALUE)
            return bytes([fc]) + struct.pack(">HH", 0, self.events)
        if fc == 0x0C:
            if data:
                raise ModbusError(EX_ILLEGAL_VALUE)
            return bytes([fc, 6]) + struct.pack(">HHH", 0, self.events, self.messages)
        if fc == 0x0F:
            if len(data) < 5:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, qty, bc = u16(data, 0), u16(data, 2), data[4]
            span(address, qty, 1968)
            self.check(address, qty)
            if bc != (qty + 7) // 8 or len(data) != 5 + bc:
                raise ModbusError(EX_ILLEGAL_VALUE)
            for i, value in enumerate(unpack_bits(data[5:], qty)):
                self.coils[address + i] = value
            return bytes([fc]) + struct.pack(">HH", address, qty)
        if fc == 0x10:
            if len(data) < 5:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, qty, bc = u16(data, 0), u16(data, 2), data[4]
            span(address, qty, 123)
            self.check(address, qty)
            if bc != qty * 2 or len(data) != 5 + bc:
                raise ModbusError(EX_ILLEGAL_VALUE)
            for i in range(qty):
                self.holding[address + i] = u16(data, 5 + i * 2)
            return bytes([fc]) + struct.pack(">HH", address, qty)
        if fc == 0x11:
            if data:
                raise ModbusError(EX_ILLEGAL_VALUE)
            text = b"TauTerm Python Test Server"
            return bytes([fc, len(text) + 2, 1, 0xFF]) + text
        if fc == 0x14:
            if not data or data[0] != len(data) - 1 or data[0] % 7:
                raise ModbusError(EX_ILLEGAL_VALUE)
            out = bytearray()
            pos = 1
            while pos < len(data):
                if data[pos] != 6:
                    raise ModbusError(EX_ILLEGAL_VALUE)
                fno, rno, qty = u16(data, pos + 1), u16(data, pos + 3), u16(data, pos + 5)
                if not 1 <= qty <= 125:
                    raise ModbusError(EX_ILLEGAL_VALUE)
                vals = [self.files.get((fno, rno + i), (fno + rno + i) & 0xFFFF) for i in range(qty)]
                sub = bytes([6]) + b"".join(struct.pack(">H", v) for v in vals)
                out += bytes([len(sub)]) + sub
                pos += 7
            return bytes([fc, len(out)]) + out
        if fc == 0x15:
            if not data or data[0] != len(data) - 1:
                raise ModbusError(EX_ILLEGAL_VALUE)
            pos = 1
            pending = []
            while pos < len(data):
                if pos + 7 > len(data) or data[pos] != 6:
                    raise ModbusError(EX_ILLEGAL_VALUE)
                fno, rno, qty = u16(data, pos + 1), u16(data, pos + 3), u16(data, pos + 5)
                end = pos + 7 + qty * 2
                if qty < 1 or end > len(data):
                    raise ModbusError(EX_ILLEGAL_VALUE)
                pending.append((fno, rno, [u16(data, pos + 7 + i * 2) for i in range(qty)]))
                pos = end
            if pos != len(data):
                raise ModbusError(EX_ILLEGAL_VALUE)
            for fno, rno, vals in pending:
                for i, value in enumerate(vals):
                    self.files[(fno, rno + i)] = value
            return bytes([fc]) + data
        if fc == 0x16:
            if len(data) != 6:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, am, om = u16(data, 0), u16(data, 2), u16(data, 4)
            self.check(address)
            current = self.holding_reg(address)
            self.holding[address] = (current & am) | (om & (~am & 0xFFFF))
            return bytes([fc]) + data
        if fc == 0x17:
            if len(data) < 9:
                raise ModbusError(EX_ILLEGAL_VALUE)
            ra, rq, wa, wq, bc = u16(data, 0), u16(data, 2), u16(data, 4), u16(data, 6), data[8]
            span(ra, rq, 125)
            span(wa, wq, 121)
            self.check(ra, rq)
            self.check(wa, wq)
            if bc != wq * 2 or len(data) != 9 + bc:
                raise ModbusError(EX_ILLEGAL_VALUE)
            for i in range(wq):
                self.holding[wa + i] = u16(data, 9 + i * 2)
            vals = [self.holding_reg(ra + i) for i in range(rq)]
            return bytes([fc, rq * 2]) + b"".join(struct.pack(">H", v) for v in vals)
        if fc == 0x18:
            if len(data) != 2:
                raise ModbusError(EX_ILLEGAL_VALUE)
            vals = self.fifo.get(u16(data, 0))
            if vals is None:
                raise ModbusError(EX_ILLEGAL_ADDRESS)
            payload = struct.pack(">H", len(vals)) + b"".join(struct.pack(">H", v) for v in vals)
            return bytes([fc]) + struct.pack(">H", len(payload)) + payload
        if fc == 0x2B:
            if len(data) != 3 or data[0] != 0x0E or not 1 <= data[1] <= 4:
                raise ModbusError(EX_ILLEGAL_FUNCTION if not data or data[0] != 0x0E else EX_ILLEGAL_VALUE)
            code, obj = data[1], data[2]
            objects = {0: b"TauTerm", 1: b"Python Test Server", 2: b"1.0"}
            if code == 4:
                if obj not in objects:
                    raise ModbusError(EX_ILLEGAL_ADDRESS)
                selected = [(obj, objects[obj])]
            else:
                selected = [item for item in objects.items() if item[0] >= obj][:16]
            body = bytearray([0x0E, code, 0x81, 0, 0, len(selected)])
            for key, value in selected:
                body += bytes([key, len(value)]) + value
            return bytes([fc]) + body
        raise ModbusError(EX_ILLEGAL_FUNCTION)


def encode_rtu(unit: int, pdu: bytes) -> bytes:
    body = bytes([unit]) + pdu
    check = crc16(body)
    return body + bytes([check & 0xFF, check >> 8])


def decode_rtu(frame: bytes) -> tuple[int, bytes]:
    if len(frame) < 4:
        raise ValueError("RTU 帧过短")
    got = frame[-2] | frame[-1] << 8
    expected = crc16(frame[:-2])
    if got != expected:
        raise ValueError(f"CRC 错误 got=0x{got:04X} expected=0x{expected:04X}")
    return frame[0], frame[1:-2]


def encode_ascii(unit: int, pdu: bytes) -> bytes:
    body = bytes([unit]) + pdu
    raw = body + bytes([lrc(body)])
    return b":" + binascii.hexlify(raw).upper() + b"\r\n"


def decode_ascii(frame: bytes) -> tuple[int, bytes]:
    if not frame.startswith(b":") or not frame.endswith(b"\r\n"):
        raise ValueError("ASCII envelope 错误")
    try:
        raw = binascii.unhexlify(frame[1:-2])
    except binascii.Error as exc:
        raise ValueError("ASCII hex 错误") from exc
    if len(raw) < 3 or lrc(raw[:-1]) != raw[-1]:
        raise ValueError("LRC 错误")
    return raw[0], raw[1:-1]


def encode_tcp(tid: int, unit: int, pdu: bytes, length_delta: int = 0) -> bytes:
    return struct.pack(">HHHB", tid & 0xFFFF, 0, (1 + len(pdu) + length_delta) & 0xFFFF, unit) + pdu


def exact(sock: socket.socket, size: int) -> bytes:
    out = bytearray()
    while len(out) < size:
        chunk = sock.recv(size - len(out))
        if not chunk:
            raise ConnectionError("对端关闭连接")
        out += chunk
    return bytes(out)


def recv_tcp(sock: socket.socket) -> tuple[int, int, bytes]:
    header = exact(sock, 7)
    tid, pid, length, unit = struct.unpack(">HHHB", header)
    if pid != 0 or not 2 <= length <= 254:
        raise ValueError(f"非法 MBAP pid={pid} length={length}")
    return tid, unit, exact(sock, length - 1)


@dataclass
class Faults:
    delay_ms: int = 0
    drop_every: int = 0
    exception: Optional[int] = None
    fragment: int = 0
    bad_tid: int = 0
    bad_length: int = 0
    truncate: int = 0
    close: int = 0

    @staticmethod
    def hit(every: int, seq: int) -> bool:
        return every > 0 and seq % every == 0


class Server:
    def __init__(self, args: argparse.Namespace):
        self.a = args
        self.model = Model(args.defined)
        self.seq = 0
        self.lock = threading.Lock()
        self.stop = threading.Event()
        self.f = Faults(args.delay_ms, args.drop_every, args.force_exception, args.fragment_size, args.bad_tid_every, args.bad_length_every, args.truncate_every, args.close_every)

    def next(self) -> int:
        with self.lock:
            self.seq += 1
            return self.seq

    def process(self, unit: int, pdu: bytes, serial: bool, seq: int) -> Optional[bytes]:
        if self.f.delay_ms:
            time.sleep(self.f.delay_ms / 1000)
        broadcast = serial and unit == 0
        if not broadcast and unit != self.a.unit:
            print(f"[忽略 #{seq}] Unit={unit}")
            return None
        drop = self.f.hit(self.f.drop_every, seq)
        fc = pdu[0] if pdu else 0
        if self.f.exception is not None and not drop and not broadcast:
            response = bytes([fc | 0x80, self.f.exception])
            status = f"FORCED EX 0x{self.f.exception:02X}"
        else:
            try:
                response = self.model.execute(pdu, serial)
                status = "OK"
            except ModbusError as exc:
                response = bytes([fc | 0x80, exc.code])
                status = f"EX 0x{exc.code:02X}"
        if broadcast:
            print(f"[广播 #{seq}] FC=0x{fc:02X} {status}，不响应")
            return None
        if drop:
            print(f"[故障 #{seq}] 无响应（请求已执行）")
            return None
        print(f"[请求 #{seq}] Unit={unit} FC=0x{fc:02X} {status} RX={hx(pdu)} TX={hx(response)}")
        return response

    def run_tcp(self) -> None:
        with socket.socket() as listener:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind((self.a.host, self.a.port))
            listener.listen()
            listener.settimeout(0.5)
            print(f"[就绪] Modbus TCP {self.a.host}:{self.a.port} Unit={self.a.unit}；Ctrl-C 退出")
            try:
                while True:
                    try:
                        conn, peer = listener.accept()
                    except socket.timeout:
                        continue
                    threading.Thread(target=self.peer, args=(conn, peer), daemon=True).start()
            except KeyboardInterrupt:
                self.stop.set()
                print(f"[结束] 请求数={self.seq}")

    def peer(self, conn: socket.socket, peer) -> None:
        with conn:
            conn.settimeout(30)
            print(f"[连接] {peer}")
            while not self.stop.is_set():
                try:
                    tid, unit, pdu = recv_tcp(conn)
                except Exception as exc:
                    print(f"[断开] {peer}: {exc}")
                    return
                seq = self.next()
                response = self.process(unit, pdu, False, seq)
                if response is None:
                    continue
                if self.f.hit(self.f.close, seq):
                    print(f"[故障 #{seq}] 主动断连")
                    return
                response_tid = (tid + 1) & 0xFFFF if self.f.hit(self.f.bad_tid, seq) else tid
                delta = 1 if self.f.hit(self.f.bad_length, seq) else 0
                adu = encode_tcp(response_tid, unit, response, delta)
                if self.f.hit(self.f.truncate, seq):
                    adu = adu[:-1]
                try:
                    step = self.f.fragment or len(adu)
                    for pos in range(0, len(adu), step):
                        conn.sendall(adu[pos:pos + step])
                        time.sleep(0.01 if self.f.fragment else 0)
                except OSError as exc:
                    print(f"[发送失败] {exc}")
                    return

    def run_serial(self, mode: str) -> None:
        try:
            import serial  # type: ignore
        except ImportError:
            raise SystemExit("串行模式需要 pyserial：pip install pyserial")
        with serial.Serial(self.a.serial_port, self.a.baud, bytesize=self.a.data_bits, parity=self.a.parity, stopbits=self.a.stop_bits, timeout=0.02, write_timeout=1) as port:
            print(f"[就绪] Modbus {mode.upper()} {self.a.serial_port} {self.a.baud}bps Unit={self.a.unit}")
            try:
                self.ascii_loop(port) if mode == "ascii" else self.rtu_loop(port)
            except KeyboardInterrupt:
                print(f"[结束] 请求数={self.seq}")

    def ascii_loop(self, port) -> None:
        buffer = bytearray()
        while True:
            chunk = port.read(4096)
            if chunk:
                buffer += chunk
            while b"\r\n" in buffer:
                end = buffer.index(b"\r\n") + 2
                frame = bytes(buffer[:end])
                del buffer[:end]
                start = frame.rfind(b":")
                if start < 0:
                    continue
                try:
                    unit, pdu = decode_ascii(frame[start:])
                except ValueError as exc:
                    print(f"[畸形] {exc}")
                    continue
                response = self.process(unit, pdu, True, self.next())
                if response is not None:
                    port.write(encode_ascii(unit, response))
                    port.flush()

    def rtu_loop(self, port) -> None:
        buffer = bytearray()
        last = 0.0
        gap = self.a.rtu_gap_ms / 1000
        while True:
            chunk = port.read(4096)
            now = time.monotonic()
            if chunk:
                buffer += chunk
                last = now
            elif buffer and now - last >= gap:
                frame = bytes(buffer)
                buffer.clear()
                try:
                    unit, pdu = decode_rtu(frame)
                except ValueError as exc:
                    print(f"[畸形] {exc}")
                    continue
                response = self.process(unit, pdu, True, self.next())
                if response is not None:
                    port.write(encode_rtu(unit, response))
                    port.flush()


def tcp_exchange(a: argparse.Namespace, pdu: bytes, tid: int) -> bytes:
    with socket.create_connection((a.host, a.port), timeout=a.timeout) as sock:
        sock.settimeout(a.timeout)
        sock.sendall(encode_tcp(tid, a.unit, pdu))
        response_tid, unit, response = recv_tcp(sock)
        if response_tid != tid:
            raise AssertionError(f"TID {response_tid} != {tid}")
        if unit != a.unit:
            raise AssertionError(f"Unit {unit} != {a.unit}")
        return response


def serial_exchange(a: argparse.Namespace, mode: str, pdu: bytes) -> bytes:
    try:
        import serial  # type: ignore
    except ImportError:
        raise SystemExit("串行模式需要 pyserial：pip install pyserial")
    with serial.Serial(a.serial_port, a.baud, bytesize=a.data_bits, parity=a.parity, stopbits=a.stop_bits, timeout=0.05, write_timeout=1) as port:
        port.reset_input_buffer()
        port.write(encode_rtu(a.unit, pdu) if mode == "rtu" else encode_ascii(a.unit, pdu))
        port.flush()
        if mode == "ascii":
            unit, response = decode_ascii(port.read_until(b"\r\n"))
        else:
            buffer = bytearray()
            deadline = time.monotonic() + a.timeout
            last = None
            while time.monotonic() < deadline:
                chunk = port.read(4096)
                if chunk:
                    buffer += chunk
                    last = time.monotonic()
                elif buffer and last and time.monotonic() - last >= a.rtu_gap_ms / 1000:
                    break
            unit, response = decode_rtu(bytes(buffer))
        if unit != a.unit:
            raise AssertionError("Unit 不匹配")
        return response


def req(fc: int, *words: int) -> bytes:
    return bytes([fc]) + b"".join(struct.pack(">H", word & 0xFFFF) for word in words)


def ok(response: bytes, fc: int) -> None:
    if len(response) >= 2 and response[0] == (fc | 0x80):
        raise AssertionError(f"异常 0x{response[1]:02X}")
    if not response or response[0] != fc:
        raise AssertionError(hx(response))


def equals(expected: bytes) -> Callable[[bytes], None]:
    def check(response: bytes) -> None:
        if response != expected:
            raise AssertionError(f"got {hx(response)} expected {hx(expected)}")
    return check


def common() -> list[tuple[str, bytes, Callable[[bytes], None]]]:
    bits = [True, False, True, True, False, False, True, False, True]
    packed = pack_bits(bits)
    write_reg = req(6, 0, 0x1234)
    write_coil = req(5, 1, 0xFF00)
    def readback(response: bytes) -> None:
        ok(response, 3)
        assert len(response) == 4 and u16(response, 2) == 0x1234
    return [
        ("FC03 Holding", req(3, 0, 4), lambda r: ok(r, 3)),
        ("FC04 Input", req(4, 0, 4), lambda r: ok(r, 4)),
        ("FC01 Coils", req(1, 0, 9), lambda r: ok(r, 1)),
        ("FC02 Discrete", req(2, 0, 9), lambda r: ok(r, 2)),
        ("FC06 Write register", write_reg, equals(write_reg)),
        ("FC03 Readback", req(3, 0, 1), readback),
        ("FC05 Write coil", write_coil, equals(write_coil)),
        ("FC0F Multi coils", bytes([0x0F]) + struct.pack(">HHB", 2, len(bits), len(packed)) + packed, lambda r: ok(r, 0x0F)),
        ("FC10 Multi regs", bytes([0x10]) + struct.pack(">HHBHHH", 1, 3, 6, 0x1111, 0x2222, 0xFFFF), lambda r: ok(r, 0x10)),
        ("FC16 Mask", req(0x16, 1, 0xFF00, 0x005A), lambda r: ok(r, 0x16)),
        ("FC17 Read/Write", bytes([0x17]) + struct.pack(">HHHHBHH", 0, 3, 4, 2, 4, 0xAAAA, 0x5555), lambda r: ok(r, 0x17)),
    ]


def advanced(serial: bool) -> list[tuple[str, bytes, Callable[[bytes], None]]]:
    tests = [
        ("FC14 File read", bytes.fromhex("14 07 06 00 01 00 00 00 02"), lambda r: ok(r, 0x14)),
        ("FC15 File write", bytes.fromhex("15 0B 06 00 01 00 00 00 02 12 34 56 78"), lambda r: ok(r, 0x15)),
        ("FC18 FIFO", req(0x18, 0), lambda r: ok(r, 0x18)),
        ("FC2B/0E Device ID", bytes.fromhex("2B 0E 01 00"), lambda r: ok(r, 0x2B)),
    ]
    if serial:
        tests += [
            ("FC07 Status", b"\x07", lambda r: ok(r, 7)),
            ("FC08 Diagnostic", bytes.fromhex("08 00 00 BE EF"), lambda r: ok(r, 8)),
            ("FC0B Counter", b"\x0B", lambda r: ok(r, 0x0B)),
            ("FC0C Log", b"\x0C", lambda r: ok(r, 0x0C)),
            ("FC11 Server ID", b"\x11", lambda r: ok(r, 0x11)),
        ]
    return tests


def run_client(a: argparse.Namespace) -> int:
    tests = common() if a.suite == "common" else advanced(a.mode != "tcp") if a.suite == "advanced" else common() + advanced(a.mode != "tcp")
    failures = 0
    tid = a.tid
    for name, pdu, check in tests:
        try:
            response = tcp_exchange(a, pdu, tid) if a.mode == "tcp" else serial_exchange(a, a.mode, pdu)
            tid = (tid + 1) & 0xFFFF
            check(response)
            print(f"[PASS] {name:<22} RX={hx(response)}")
        except Exception as exc:
            failures += 1
            print(f"[FAIL] {name:<22} {exc}")
    print(f"[结果] PASS={len(tests) - failures} FAIL={failures} TOTAL={len(tests)}")
    return 1 if failures else 0


def self_test() -> int:
    assert crc16(bytes.fromhex("01 03 00 00 00 0A")) == 0xCDC5
    pdu = bytes.fromhex("03 00 00 00 02")
    assert decode_rtu(encode_rtu(1, pdu)) == (1, pdu)
    assert decode_ascii(encode_ascii(1, pdu)) == (1, pdu)
    assert encode_tcp(0x1234, 1, pdu)[:7] == bytes.fromhex("12 34 00 00 00 06 01")
    model = Model()
    write = req(6, 0, 0x1234)
    assert model.execute(write, False) == write
    assert model.execute(req(3, 0, 1), False) == bytes.fromhex("03 02 12 34")
    try:
        model.execute(b"\x07", False)
    except ModbusError as exc:
        assert exc.code == EX_ILLEGAL_FUNCTION
    else:
        raise AssertionError("FC07 必须仅串行")
    assert model.execute(bytes.fromhex("2B 0E 01 00"), False)[:3] == bytes.fromhex("2B 0E 01")
    print("Modbus test fixture self-test passed.")
    return 0


def add_serial(p: argparse.ArgumentParser) -> None:
    p.add_argument("--serial-port", default="COM200")
    p.add_argument("--baud", type=int, default=115200)
    p.add_argument("--data-bits", type=int, choices=(7, 8), default=8)
    p.add_argument("--parity", choices=("N", "E", "O"), default="N")
    p.add_argument("--stop-bits", type=float, choices=(1, 2), default=1)
    p.add_argument("--rtu-gap-ms", type=float, default=4.0)


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="TauTerm Modbus RTU/ASCII/TCP 完整测试夹具")
    p.add_argument("--self-test", action="store_true")
    sub = p.add_subparsers(dest="command")
    server = sub.add_parser("server")
    server.add_argument("mode", choices=("tcp", "rtu", "ascii"))
    server.add_argument("--unit", type=n, default=1)
    server.add_argument("--defined", type=int, default=256)
    server.add_argument("--host", default="127.0.0.1")
    server.add_argument("--port", type=int, default=1502)
    add_serial(server)
    server.add_argument("--delay-ms", type=int, default=0)
    server.add_argument("--drop-every", type=int, default=0)
    server.add_argument("--force-exception", type=n)
    server.add_argument("--fragment-size", type=int, default=0)
    server.add_argument("--bad-tid-every", type=int, default=0)
    server.add_argument("--bad-length-every", type=int, default=0)
    server.add_argument("--truncate-every", type=int, default=0)
    server.add_argument("--close-every", type=int, default=0)
    client = sub.add_parser("client")
    client.add_argument("mode", choices=("tcp", "rtu", "ascii"))
    client.add_argument("--unit", type=n, default=1)
    client.add_argument("--host", default="127.0.0.1")
    client.add_argument("--port", type=int, default=1502)
    client.add_argument("--timeout", type=float, default=2.0)
    client.add_argument("--tid", type=n, default=1)
    client.add_argument("--suite", choices=("common", "advanced", "all"), default="common")
    add_serial(client)
    return p


def main() -> int:
    p = parser()
    args = p.parse_args()
    if args.self_test:
        return self_test()
    if not args.command:
        p.print_help()
        return 2
    if not 0 <= args.unit <= 255:
        p.error("--unit 必须为 0..255")
    if args.command == "server":
        server = Server(args)
        server.run_tcp() if args.mode == "tcp" else server.run_serial(args.mode)
        return 0
    return run_client(args)


if __name__ == "__main__":
    raise SystemExit(main())
