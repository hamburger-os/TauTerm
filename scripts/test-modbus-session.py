#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
TauTerm Modbus 会话完整人工回归测试夹具。

用途：
  1) TauTerm Client -> 本脚本 Server：验证 RTU / ASCII / TCP、功能码、异常、
     广播、TCP 分片以及协议/传输故障恢复。
  2) 本脚本 Client -> TauTerm Server Simulator：验证标准/高级功能、异常响应、
     串行广播，以及 TCP 对畸形 MBAP 的防御行为。

快速开始：
  python scripts/test-modbus-session.py --self-test
  python scripts/test-modbus-session.py server tcp --host 127.0.0.1 --port 1502
  python scripts/test-modbus-session.py client tcp --host 127.0.0.1 --port 1502 --suite all

串口：RTU/ASCII 需要 pyserial。Windows 可与 test-serial-session.py 相同，使用
COM200 <-> COM201 虚拟串口对；脚本使用一端，TauTerm 使用另一端。

Server 覆盖：FC01/02/03/04/05/06/0F/10/16/17，串行专属 FC07/08/0B/0C/11，
高级 FC14/15/18/2B-0E；CRC/LRC/MBAP/TID；串行广播；延时/无响应/强制异常；
错误 TID、错误 MBAP Length、截断响应、主动断连和 TCP 响应分片。

Client suite：
  common     常用功能码成功路径
  advanced   高级功能码；串行时额外覆盖 07/08/0B/0C/11
  negative   Illegal Function / Address / Value；TCP 额外验证非法 PID/MBAP Length 断连
  broadcast  RTU/ASCII 广播写 + 无响应 + 普通 Unit 读回确认（TCP 不适用）
  all        common + advanced + negative；串行再加 broadcast

故障示例：
  --delay-ms 800           延迟响应
  --drop-every 3           每 3 个请求无响应（请求仍执行）
  --force-exception 0x04   强制异常（不执行真实写）
  --bad-tid-every 2        错误 TID
  --bad-length-every 2     错误 MBAP Length
  --truncate-every 2       截断响应
  --close-every 2          响应前主动断连
  --fragment-size 3        TCP 每 3 字节分片发送

注意：TauTerm Server Simulator 的协议写只能修改已在工作台定义的地址。使用本脚本
client 测 TauTerm Server 前，请至少定义 coils/discrete/holding/input 的 0..31 地址。
广播 suite 会写 Holding Register 0，然后以正常 Unit 读回确认。
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
WRITE_FUNCTIONS = {0x05, 0x06, 0x0F, 0x10, 0x15, 0x16, 0x17}
EX_ILLEGAL_FUNCTION = 0x01
EX_ILLEGAL_ADDRESS = 0x02
EX_ILLEGAL_VALUE = 0x03


def number(value: str) -> int:
    return int(value, 0)


def hx(data: bytes) -> str:
    return " ".join(f"{byte:02X}" for byte in data)


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
    for index, value in enumerate(values):
        if value:
            out[index // 8] |= 1 << (index % 8)
    return bytes(out)


def unpack_bits(data: bytes, count: int) -> list[bool]:
    return [bool(data[index // 8] & (1 << (index % 8))) for index in range(count)]


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
    fifo: dict[int, list[int]] = field(
        default_factory=lambda: {0: [0x1111, 0x2222, 0x3333]}
    )
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
        if fc in (0x01, 0x02):
            if len(data) != 4:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, quantity = u16(data, 0), u16(data, 2)
            span(address, quantity, 2000)
            self.check(address, quantity)
            values = [
                self.coil(address + i) if fc == 0x01 else (address + i) % 2 == 1
                for i in range(quantity)
            ]
            packed = pack_bits(values)
            return bytes([fc, len(packed)]) + packed

        if fc in (0x03, 0x04):
            if len(data) != 4:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, quantity = u16(data, 0), u16(data, 2)
            span(address, quantity, 125)
            self.check(address, quantity)
            values = [
                self.holding_reg(address + i)
                if fc == 0x03
                else (0x2000 + 29 * (address + i)) & 0xFFFF
                for i in range(quantity)
            ]
            return bytes([fc, quantity * 2]) + b"".join(
                struct.pack(">H", value) for value in values
            )

        if fc == 0x05:
            if len(data) != 4:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, raw = u16(data, 0), u16(data, 2)
            self.check(address)
            if raw not in (0x0000, 0xFF00):
                raise ModbusError(EX_ILLEGAL_VALUE)
            self.coils[address] = raw == 0xFF00
            return bytes([fc]) + data

        if fc == 0x06:
            if len(data) != 4:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, value = u16(data, 0), u16(data, 2)
            self.check(address)
            self.holding[address] = value
            return bytes([fc]) + data

        if fc == 0x07:
            if data:
                raise ModbusError(EX_ILLEGAL_VALUE)
            return bytes([fc, self.exception_status])

        if fc == 0x08:
            if len(data) < 4 or len(data) % 2:
                raise ModbusError(EX_ILLEGAL_VALUE)
            sub_function = u16(data, 0)
            if sub_function == 0x0000:
                return bytes([fc]) + data
            if sub_function == 0x000A and data[2:] == b"\x00\x00":
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
            address, quantity, byte_count = u16(data, 0), u16(data, 2), data[4]
            span(address, quantity, 1968)
            self.check(address, quantity)
            if byte_count != (quantity + 7) // 8 or len(data) != 5 + byte_count:
                raise ModbusError(EX_ILLEGAL_VALUE)
            for index, value in enumerate(unpack_bits(data[5:], quantity)):
                self.coils[address + index] = value
            return bytes([fc]) + struct.pack(">HH", address, quantity)

        if fc == 0x10:
            if len(data) < 5:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address, quantity, byte_count = u16(data, 0), u16(data, 2), data[4]
            span(address, quantity, 123)
            self.check(address, quantity)
            if byte_count != quantity * 2 or len(data) != 5 + byte_count:
                raise ModbusError(EX_ILLEGAL_VALUE)
            for index in range(quantity):
                self.holding[address + index] = u16(data, 5 + index * 2)
            return bytes([fc]) + struct.pack(">HH", address, quantity)

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
                if data[pos] != 0x06:
                    raise ModbusError(EX_ILLEGAL_VALUE)
                file_no = u16(data, pos + 1)
                record_no = u16(data, pos + 3)
                quantity = u16(data, pos + 5)
                if not 1 <= quantity <= 125:
                    raise ModbusError(EX_ILLEGAL_VALUE)
                values = [
                    self.files.get(
                        (file_no, record_no + index),
                        (file_no + record_no + index) & 0xFFFF,
                    )
                    for index in range(quantity)
                ]
                sub = bytes([0x06]) + b"".join(
                    struct.pack(">H", value) for value in values
                )
                out += bytes([len(sub)]) + sub
                pos += 7
            return bytes([fc, len(out)]) + out

        if fc == 0x15:
            if not data or data[0] != len(data) - 1:
                raise ModbusError(EX_ILLEGAL_VALUE)
            pos = 1
            pending: list[tuple[int, int, list[int]]] = []
            while pos < len(data):
                if pos + 7 > len(data) or data[pos] != 0x06:
                    raise ModbusError(EX_ILLEGAL_VALUE)
                file_no = u16(data, pos + 1)
                record_no = u16(data, pos + 3)
                quantity = u16(data, pos + 5)
                end = pos + 7 + quantity * 2
                if quantity < 1 or end > len(data):
                    raise ModbusError(EX_ILLEGAL_VALUE)
                values = [u16(data, pos + 7 + index * 2) for index in range(quantity)]
                pending.append((file_no, record_no, values))
                pos = end
            if pos != len(data):
                raise ModbusError(EX_ILLEGAL_VALUE)
            for file_no, record_no, values in pending:
                for index, value in enumerate(values):
                    self.files[(file_no, record_no + index)] = value
            return bytes([fc]) + data

        if fc == 0x16:
            if len(data) != 6:
                raise ModbusError(EX_ILLEGAL_VALUE)
            address = u16(data, 0)
            and_mask = u16(data, 2)
            or_mask = u16(data, 4)
            self.check(address)
            current = self.holding_reg(address)
            self.holding[address] = (current & and_mask) | (or_mask & (~and_mask & 0xFFFF))
            return bytes([fc]) + data

        if fc == 0x17:
            if len(data) < 9:
                raise ModbusError(EX_ILLEGAL_VALUE)
            read_address = u16(data, 0)
            read_quantity = u16(data, 2)
            write_address = u16(data, 4)
            write_quantity = u16(data, 6)
            byte_count = data[8]
            span(read_address, read_quantity, 125)
            span(write_address, write_quantity, 121)
            self.check(read_address, read_quantity)
            self.check(write_address, write_quantity)
            if byte_count != write_quantity * 2 or len(data) != 9 + byte_count:
                raise ModbusError(EX_ILLEGAL_VALUE)
            for index in range(write_quantity):
                self.holding[write_address + index] = u16(data, 9 + index * 2)
            values = [
                self.holding_reg(read_address + index)
                for index in range(read_quantity)
            ]
            return bytes([fc, read_quantity * 2]) + b"".join(
                struct.pack(">H", value) for value in values
            )

        if fc == 0x18:
            if len(data) != 2:
                raise ModbusError(EX_ILLEGAL_VALUE)
            values = self.fifo.get(u16(data, 0))
            if values is None:
                raise ModbusError(EX_ILLEGAL_ADDRESS)
            payload = struct.pack(">H", len(values)) + b"".join(
                struct.pack(">H", value) for value in values
            )
            return bytes([fc]) + struct.pack(">H", len(payload)) + payload

        if fc == 0x2B:
            if len(data) != 3 or data[0] != 0x0E or not 1 <= data[1] <= 4:
                raise ModbusError(
                    EX_ILLEGAL_FUNCTION if not data or data[0] != 0x0E else EX_ILLEGAL_VALUE
                )
            read_code, object_id = data[1], data[2]
            objects = {0: b"TauTerm", 1: b"Python Test Server", 2: b"1.0"}
            if read_code == 4:
                if object_id not in objects:
                    raise ModbusError(EX_ILLEGAL_ADDRESS)
                selected = [(object_id, objects[object_id])]
            else:
                selected = [item for item in objects.items() if item[0] >= object_id][:16]
            body = bytearray([0x0E, read_code, 0x81, 0, 0, len(selected)])
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
    except (binascii.Error, ValueError) as exc:
        raise ValueError("ASCII hex 错误") from exc
    if len(raw) < 3 or lrc(raw[:-1]) != raw[-1]:
        raise ValueError("LRC 错误")
    return raw[0], raw[1:-1]


def encode_tcp(
    tid: int,
    unit: int,
    pdu: bytes,
    *,
    protocol_id: int = 0,
    length_delta: int = 0,
) -> bytes:
    length = (1 + len(pdu) + length_delta) & 0xFFFF
    return struct.pack(">HHHB", tid & 0xFFFF, protocol_id & 0xFFFF, length, unit) + pdu


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
    tid, protocol_id, length, unit = struct.unpack(">HHHB", header)
    if protocol_id != 0 or not 2 <= length <= 254:
        raise ValueError(f"非法 MBAP pid={protocol_id} length={length}")
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
    def hit(every: int, sequence: int) -> bool:
        return every > 0 and sequence % every == 0


class Server:
    def __init__(self, args: argparse.Namespace):
        self.args = args
        self.model = Model(args.defined)
        self.sequence = 0
        self.sequence_lock = threading.Lock()
        self.model_lock = threading.Lock()
        self.stop = threading.Event()
        self.faults = Faults(
            args.delay_ms,
            args.drop_every,
            args.force_exception,
            args.fragment_size,
            args.bad_tid_every,
            args.bad_length_every,
            args.truncate_every,
            args.close_every,
        )

    def next_sequence(self) -> int:
        with self.sequence_lock:
            self.sequence += 1
            return self.sequence

    def process(self, unit: int, pdu: bytes, serial: bool, sequence: int) -> Optional[bytes]:
        if self.faults.delay_ms:
            time.sleep(self.faults.delay_ms / 1000.0)
        broadcast = serial and unit == 0
        if not broadcast and unit != self.args.unit:
            print(f"[忽略 #{sequence}] Unit={unit}")
            return None
        function = pdu[0] if pdu else 0
        if broadcast and function not in WRITE_FUNCTIONS:
            print(f"[广播 #{sequence}] FC=0x{function:02X} 非写操作，按标准忽略且不响应")
            return None

        drop = self.faults.hit(self.faults.drop_every, sequence)
        if self.faults.exception is not None and not drop:
            response = bytes([function | 0x80, self.faults.exception])
            status = f"FORCED EX 0x{self.faults.exception:02X}"
        else:
            try:
                with self.model_lock:
                    response = self.model.execute(pdu, serial)
                status = "OK"
            except ModbusError as exc:
                response = bytes([function | 0x80, exc.code])
                status = f"EX 0x{exc.code:02X}"

        if broadcast:
            print(f"[广播 #{sequence}] FC=0x{function:02X} {status}，不响应")
            return None
        if drop:
            print(f"[故障 #{sequence}] 无响应（请求已执行）")
            return None
        print(
            f"[请求 #{sequence}] Unit={unit} FC=0x{function:02X} {status} "
            f"RX={hx(pdu)} TX={hx(response)}"
        )
        return response

    def run_tcp(self) -> None:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind((self.args.host, self.args.port))
            listener.listen()
            listener.settimeout(0.5)
            print(
                f"[就绪] Modbus TCP {self.args.host}:{self.args.port} "
                f"Unit={self.args.unit}；Ctrl-C 退出"
            )
            try:
                while not self.stop.is_set():
                    try:
                        connection, peer = listener.accept()
                    except socket.timeout:
                        continue
                    threading.Thread(
                        target=self.tcp_peer,
                        args=(connection, peer),
                        daemon=True,
                    ).start()
            except KeyboardInterrupt:
                self.stop.set()
                print(f"[结束] 请求数={self.sequence}")

    def tcp_peer(self, connection: socket.socket, peer) -> None:
        with connection:
            connection.settimeout(30)
            print(f"[连接] {peer}")
            while not self.stop.is_set():
                try:
                    tid, unit, pdu = recv_tcp(connection)
                except Exception as exc:
                    print(f"[断开] {peer}: {exc}")
                    return
                sequence = self.next_sequence()
                response = self.process(unit, pdu, False, sequence)
                if response is None:
                    continue
                if self.faults.hit(self.faults.close, sequence):
                    print(f"[故障 #{sequence}] 主动断连")
                    return
                response_tid = (
                    (tid + 1) & 0xFFFF
                    if self.faults.hit(self.faults.bad_tid, sequence)
                    else tid
                )
                length_delta = 1 if self.faults.hit(self.faults.bad_length, sequence) else 0
                adu = encode_tcp(response_tid, unit, response, length_delta=length_delta)
                if self.faults.hit(self.faults.truncate, sequence):
                    adu = adu[:-1]
                try:
                    step = self.faults.fragment or len(adu)
                    for pos in range(0, len(adu), step):
                        connection.sendall(adu[pos : pos + step])
                        if self.faults.fragment:
                            time.sleep(0.01)
                except OSError as exc:
                    print(f"[发送失败] {exc}")
                    return

    def run_serial(self, mode: str) -> None:
        serial = require_pyserial()
        with serial.Serial(
            self.args.serial_port,
            self.args.baud,
            bytesize=self.args.data_bits,
            parity=self.args.parity,
            stopbits=self.args.stop_bits,
            timeout=0.02,
            write_timeout=1,
        ) as port:
            print(
                f"[就绪] Modbus {mode.upper()} {self.args.serial_port} "
                f"{self.args.baud}bps Unit={self.args.unit}"
            )
            try:
                self.ascii_loop(port) if mode == "ascii" else self.rtu_loop(port)
            except KeyboardInterrupt:
                print(f"[结束] 请求数={self.sequence}")

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
                response = self.process(unit, pdu, True, self.next_sequence())
                if response is not None:
                    port.write(encode_ascii(unit, response))
                    port.flush()

    def rtu_loop(self, port) -> None:
        buffer = bytearray()
        last_byte_at = 0.0
        gap = self.args.rtu_gap_ms / 1000.0
        while True:
            chunk = port.read(4096)
            now = time.monotonic()
            if chunk:
                buffer += chunk
                last_byte_at = now
            elif buffer and now - last_byte_at >= gap:
                frame = bytes(buffer)
                buffer.clear()
                try:
                    unit, pdu = decode_rtu(frame)
                except ValueError as exc:
                    print(f"[畸形] {exc}")
                    continue
                response = self.process(unit, pdu, True, self.next_sequence())
                if response is not None:
                    port.write(encode_rtu(unit, response))
                    port.flush()


def require_pyserial():
    try:
        import serial  # type: ignore
    except ImportError as exc:
        raise SystemExit("串行模式需要 pyserial：pip install pyserial") from exc
    return serial


def tcp_exchange(args: argparse.Namespace, pdu: bytes, tid: int) -> bytes:
    with socket.create_connection((args.host, args.port), timeout=args.timeout) as sock:
        sock.settimeout(args.timeout)
        sock.sendall(encode_tcp(tid, args.unit, pdu))
        response_tid, unit, response = recv_tcp(sock)
        if response_tid != tid:
            raise AssertionError(f"TID {response_tid} != {tid}")
        if unit != args.unit:
            raise AssertionError(f"Unit {unit} != {args.unit}")
        return response


def tcp_malformed_closes(
    args: argparse.Namespace,
    *,
    protocol_id: int = 0,
    length_delta: int = 0,
) -> None:
    pdu = req(0x03, 0, 1)
    with socket.create_connection((args.host, args.port), timeout=args.timeout) as sock:
        sock.settimeout(min(args.timeout, 1.0))
        sock.sendall(
            encode_tcp(
                args.tid,
                args.unit,
                pdu,
                protocol_id=protocol_id,
                length_delta=length_delta,
            )
        )
        try:
            data = sock.recv(32)
        except (socket.timeout, ConnectionResetError, ConnectionAbortedError):
            return
        if data:
            raise AssertionError(f"畸形 MBAP 不应收到正常响应: {hx(data)}")


def serial_exchange(
    args: argparse.Namespace,
    mode: str,
    pdu: bytes,
    *,
    unit: Optional[int] = None,
    expect_response: bool = True,
) -> bytes:
    serial = require_pyserial()
    target = args.unit if unit is None else unit
    with serial.Serial(
        args.serial_port,
        args.baud,
        bytesize=args.data_bits,
        parity=args.parity,
        stopbits=args.stop_bits,
        timeout=0.05,
        write_timeout=1,
    ) as port:
        port.reset_input_buffer()
        request = encode_rtu(target, pdu) if mode == "rtu" else encode_ascii(target, pdu)
        port.write(request)
        port.flush()
        deadline = time.monotonic() + args.timeout

        if mode == "ascii":
            frame = bytearray()
            while time.monotonic() < deadline:
                chunk = port.read(1)
                if chunk:
                    frame += chunk
                    if frame.endswith(b"\r\n"):
                        break
            if not frame:
                if expect_response:
                    raise TimeoutError("等待 Modbus ASCII 响应超时")
                return b""
            if not expect_response:
                raise AssertionError(f"广播请求不应收到响应: {bytes(frame)!r}")
            response_unit, response = decode_ascii(bytes(frame))
        else:
            buffer = bytearray()
            last_byte_at: Optional[float] = None
            while time.monotonic() < deadline:
                chunk = port.read(4096)
                if chunk:
                    buffer += chunk
                    last_byte_at = time.monotonic()
                elif (
                    buffer
                    and last_byte_at is not None
                    and time.monotonic() - last_byte_at >= args.rtu_gap_ms / 1000.0
                ):
                    break
            if not buffer:
                if expect_response:
                    raise TimeoutError("等待 Modbus RTU 响应超时")
                return b""
            if not expect_response:
                raise AssertionError(f"广播请求不应收到响应: {hx(bytes(buffer))}")
            response_unit, response = decode_rtu(bytes(buffer))

        if response_unit != target:
            raise AssertionError(f"Unit {response_unit} != {target}")
        return response


def req(function: int, *words: int) -> bytes:
    return bytes([function]) + b"".join(
        struct.pack(">H", word & 0xFFFF) for word in words
    )


def expect_ok(response: bytes, function: int) -> None:
    if len(response) >= 2 and response[0] == (function | 0x80):
        raise AssertionError(f"异常 0x{response[1]:02X}")
    if not response or response[0] != function:
        raise AssertionError(hx(response))


def expect_equal(expected: bytes) -> Callable[[bytes], None]:
    def check(response: bytes) -> None:
        if response != expected:
            raise AssertionError(f"got {hx(response)} expected {hx(expected)}")

    return check


def expect_exception(function: int, code: int) -> Callable[[bytes], None]:
    expected = bytes([function | 0x80, code])
    return expect_equal(expected)


Test = tuple[str, bytes, Callable[[bytes], None]]


def common_tests() -> list[Test]:
    bits = [True, False, True, True, False, False, True, False, True]
    packed = pack_bits(bits)
    write_reg = req(0x06, 0, 0x1234)
    write_coil = req(0x05, 1, 0xFF00)

    def readback(response: bytes) -> None:
        expect_ok(response, 0x03)
        if len(response) != 4 or u16(response, 2) != 0x1234:
            raise AssertionError(f"读回值错误: {hx(response)}")

    return [
        ("FC03 Holding", req(0x03, 0, 4), lambda r: expect_ok(r, 0x03)),
        ("FC04 Input", req(0x04, 0, 4), lambda r: expect_ok(r, 0x04)),
        ("FC01 Coils", req(0x01, 0, 9), lambda r: expect_ok(r, 0x01)),
        ("FC02 Discrete", req(0x02, 0, 9), lambda r: expect_ok(r, 0x02)),
        ("FC06 Write register", write_reg, expect_equal(write_reg)),
        ("FC03 Readback", req(0x03, 0, 1), readback),
        ("FC05 Write coil", write_coil, expect_equal(write_coil)),
        (
            "FC0F Multi coils",
            bytes([0x0F]) + struct.pack(">HHB", 2, len(bits), len(packed)) + packed,
            lambda r: expect_ok(r, 0x0F),
        ),
        (
            "FC10 Multi regs",
            bytes([0x10])
            + struct.pack(">HHBHHH", 1, 3, 6, 0x1111, 0x2222, 0xFFFF),
            lambda r: expect_ok(r, 0x10),
        ),
        ("FC16 Mask", req(0x16, 1, 0xFF00, 0x005A), lambda r: expect_ok(r, 0x16)),
        (
            "FC17 Read/Write",
            bytes([0x17])
            + struct.pack(">HHHHBHH", 0, 3, 4, 2, 4, 0xAAAA, 0x5555),
            lambda r: expect_ok(r, 0x17),
        ),
    ]


def advanced_tests(serial: bool) -> list[Test]:
    tests: list[Test] = [
        (
            "FC14 File read",
            bytes.fromhex("14 07 06 00 01 00 00 00 02"),
            lambda r: expect_ok(r, 0x14),
        ),
        (
            "FC15 File write",
            bytes.fromhex("15 0B 06 00 01 00 00 00 02 12 34 56 78"),
            lambda r: expect_ok(r, 0x15),
        ),
        ("FC18 FIFO", req(0x18, 0), lambda r: expect_ok(r, 0x18)),
        (
            "FC2B/0E Device ID",
            bytes.fromhex("2B 0E 01 00"),
            lambda r: expect_ok(r, 0x2B),
        ),
    ]
    if serial:
        tests.extend(
            [
                ("FC07 Status", b"\x07", lambda r: expect_ok(r, 0x07)),
                (
                    "FC08 Diagnostic",
                    bytes.fromhex("08 00 00 BE EF"),
                    lambda r: expect_ok(r, 0x08),
                ),
                ("FC0B Counter", b"\x0B", lambda r: expect_ok(r, 0x0B)),
                ("FC0C Log", b"\x0C", lambda r: expect_ok(r, 0x0C)),
                ("FC11 Server ID", b"\x11", lambda r: expect_ok(r, 0x11)),
            ]
        )
    return tests


def negative_tests(serial: bool) -> list[Test]:
    tests: list[Test] = [
        (
            "Illegal function",
            bytes.fromhex("7F 00 00"),
            expect_exception(0x7F, EX_ILLEGAL_FUNCTION),
        ),
        (
            "Illegal address",
            req(0x03, 0xFFFF, 2),
            expect_exception(0x03, EX_ILLEGAL_ADDRESS),
        ),
        (
            "Illegal quantity",
            req(0x03, 0, 0),
            expect_exception(0x03, EX_ILLEGAL_VALUE),
        ),
        (
            "Invalid coil value",
            req(0x05, 0, 0x1234),
            expect_exception(0x05, EX_ILLEGAL_VALUE),
        ),
        (
            "Invalid MEI read code",
            bytes.fromhex("2B 0E 00 00"),
            expect_exception(0x2B, EX_ILLEGAL_VALUE),
        ),
    ]
    if not serial:
        tests.append(
            (
                "Serial-only FC07/TCP",
                b"\x07",
                expect_exception(0x07, EX_ILLEGAL_FUNCTION),
            )
        )
    return tests


def exchange(args: argparse.Namespace, pdu: bytes, tid: int) -> bytes:
    if args.mode == "tcp":
        return tcp_exchange(args, pdu, tid)
    return serial_exchange(args, args.mode, pdu)


def run_cases(args: argparse.Namespace, tests: list[Test], tid: int) -> tuple[int, int]:
    failures = 0
    for name, pdu, check in tests:
        try:
            response = exchange(args, pdu, tid)
            tid = (tid + 1) & 0xFFFF
            check(response)
            print(f"[PASS] {name:<26} RX={hx(response)}")
        except Exception as exc:
            failures += 1
            print(f"[FAIL] {name:<26} {exc}")
    return failures, tid


def run_tcp_malformed_cases(args: argparse.Namespace) -> int:
    failures = 0
    for name, kwargs in (
        ("TCP invalid Protocol ID", {"protocol_id": 1}),
        ("TCP invalid MBAP length", {"length_delta": 300}),
    ):
        try:
            tcp_malformed_closes(args, **kwargs)
            print(f"[PASS] {name:<26} peer closed/no response")
        except Exception as exc:
            failures += 1
            print(f"[FAIL] {name:<26} {exc}")
    return failures


def run_broadcast(args: argparse.Namespace) -> int:
    if args.mode == "tcp":
        print("[错误] Modbus TCP 不使用串行 Unit 0 广播语义", file=sys.stderr)
        return 2
    write = req(0x06, 0, 0xBEEF)
    try:
        response = serial_exchange(
            args,
            args.mode,
            write,
            unit=0,
            expect_response=False,
        )
        if response:
            raise AssertionError(f"广播不应有响应: {hx(response)}")
        print("[PASS] Broadcast FC06          no response")
        readback = serial_exchange(args, args.mode, req(0x03, 0, 1))
        expect_ok(readback, 0x03)
        if len(readback) != 4 or u16(readback, 2) != 0xBEEF:
            raise AssertionError(f"广播写未生效: {hx(readback)}")
        print(f"[PASS] Broadcast readback      RX={hx(readback)}")
        return 0
    except Exception as exc:
        print(f"[FAIL] Broadcast               {exc}")
        return 1


def run_client(args: argparse.Namespace) -> int:
    serial = args.mode != "tcp"
    if args.suite == "broadcast":
        return run_broadcast(args)

    selected: list[Test] = []
    if args.suite in ("common", "all"):
        selected.extend(common_tests())
    if args.suite in ("advanced", "all"):
        selected.extend(advanced_tests(serial))
    if args.suite in ("negative", "all"):
        selected.extend(negative_tests(serial))

    failures, _ = run_cases(args, selected, args.tid)
    extra = 0
    if args.mode == "tcp" and args.suite in ("negative", "all"):
        extra += run_tcp_malformed_cases(args)
    if serial and args.suite == "all":
        extra += run_broadcast(args)
    failures += extra
    total = len(selected) + (2 if args.mode == "tcp" and args.suite in ("negative", "all") else 0)
    if serial and args.suite == "all":
        total += 2
    print(f"[结果] PASS={total - failures} FAIL={failures} TOTAL={total}")
    return 1 if failures else 0


def self_test() -> int:
    assert crc16(bytes.fromhex("01 03 00 00 00 0A")) == 0xCDC5
    pdu = bytes.fromhex("03 00 00 00 02")
    assert decode_rtu(encode_rtu(1, pdu)) == (1, pdu)
    assert decode_ascii(encode_ascii(1, pdu)) == (1, pdu)
    assert encode_tcp(0x1234, 1, pdu)[:7] == bytes.fromhex("12 34 00 00 00 06 01")

    model = Model()
    write = req(0x06, 0, 0x1234)
    assert model.execute(write, False) == write
    assert model.execute(req(0x03, 0, 1), False) == bytes.fromhex("03 02 12 34")

    for request, code in (
        (b"\x07", EX_ILLEGAL_FUNCTION),
        (req(0x03, 0xFFFF, 2), EX_ILLEGAL_ADDRESS),
        (req(0x03, 0, 0), EX_ILLEGAL_VALUE),
        (req(0x05, 0, 0x1234), EX_ILLEGAL_VALUE),
    ):
        try:
            model.execute(request, False)
        except ModbusError as exc:
            assert exc.code == code
        else:
            raise AssertionError(f"expected exception 0x{code:02X} for {hx(request)}")

    assert model.execute(bytes.fromhex("2B 0E 01 00"), False)[:3] == bytes.fromhex(
        "2B 0E 01"
    )
    print("Modbus test fixture self-test passed.")
    return 0


def add_serial(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--serial-port", default="COM200")
    parser.add_argument("--baud", type=int, default=115200)
    parser.add_argument("--data-bits", type=int, choices=(7, 8), default=8)
    parser.add_argument("--parity", choices=("N", "E", "O"), default="N")
    parser.add_argument("--stop-bits", type=float, choices=(1, 2), default=1)
    parser.add_argument("--rtu-gap-ms", type=float, default=4.0)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="TauTerm Modbus RTU/ASCII/TCP 完整测试夹具",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "client --suite all 为推荐完整回归；RTU/ASCII 会自动包含广播写/读回。\n"
            "故障注入参数属于 server 子命令，用于测试 TauTerm Client 的鲁棒性。"
        ),
    )
    parser.add_argument("--self-test", action="store_true")
    sub = parser.add_subparsers(dest="command")

    server = sub.add_parser("server", help="作为 Modbus Server/Slave 测试 TauTerm Client")
    server.add_argument("mode", choices=("tcp", "rtu", "ascii"))
    server.add_argument("--unit", type=number, default=1)
    server.add_argument("--defined", type=int, default=256)
    server.add_argument("--host", default="127.0.0.1")
    server.add_argument("--port", type=int, default=1502)
    add_serial(server)
    server.add_argument("--delay-ms", type=int, default=0)
    server.add_argument("--drop-every", type=int, default=0)
    server.add_argument("--force-exception", type=number)
    server.add_argument("--fragment-size", type=int, default=0)
    server.add_argument("--bad-tid-every", type=int, default=0)
    server.add_argument("--bad-length-every", type=int, default=0)
    server.add_argument("--truncate-every", type=int, default=0)
    server.add_argument("--close-every", type=int, default=0)

    client = sub.add_parser("client", help="作为 Modbus Client/Master 测试 TauTerm Server Simulator")
    client.add_argument("mode", choices=("tcp", "rtu", "ascii"))
    client.add_argument("--unit", type=number, default=1)
    client.add_argument("--host", default="127.0.0.1")
    client.add_argument("--port", type=int, default=1502)
    client.add_argument("--timeout", type=float, default=2.0)
    client.add_argument("--tid", type=number, default=1)
    client.add_argument(
        "--suite",
        choices=("common", "advanced", "negative", "broadcast", "all"),
        default="common",
    )
    add_serial(client)
    return parser


def validate_args(parser: argparse.ArgumentParser, args: argparse.Namespace) -> None:
    if args.mode == "tcp":
        if not 0 <= args.unit <= 255:
            parser.error("Modbus TCP --unit 必须为 0..255")
        if not 1 <= args.port <= 65535:
            parser.error("--port 必须为 1..65535")
    else:
        if not 1 <= args.unit <= 247:
            parser.error("RTU/ASCII 普通 Unit 必须为 1..247；广播由 --suite broadcast/all 自动使用 Unit 0")
        if args.rtu_gap_ms <= 0:
            parser.error("--rtu-gap-ms 必须 > 0")
    if args.command == "server":
        if args.defined < 1 or args.defined > 65536:
            parser.error("--defined 必须为 1..65536")
        if args.delay_ms < 0:
            parser.error("--delay-ms 必须 >= 0")
        if args.force_exception is not None and not 1 <= args.force_exception <= 0xFF:
            parser.error("--force-exception 必须为 0x01..0xFF")
        for name in (
            "drop_every",
            "fragment_size",
            "bad_tid_every",
            "bad_length_every",
            "truncate_every",
            "close_every",
        ):
            if getattr(args, name) < 0:
                parser.error(f"--{name.replace('_', '-')} 必须 >= 0")
    else:
        if args.timeout <= 0:
            parser.error("--timeout 必须 > 0")
        if not 0 <= args.tid <= 0xFFFF:
            parser.error("--tid 必须为 0..65535")
        if args.mode == "tcp" and args.suite == "broadcast":
            parser.error("broadcast suite 仅适用于 RTU/ASCII")


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if not args.command:
        parser.print_help()
        return 2
    validate_args(parser, args)
    if args.command == "server":
        server = Server(args)
        server.run_tcp() if args.mode == "tcp" else server.run_serial(args.mode)
        return 0
    return run_client(args)


if __name__ == "__main__":
    raise SystemExit(main())
