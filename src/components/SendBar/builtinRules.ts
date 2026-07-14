/**
 * 内置自动应答示例配置。
 *
 * 10 套配置覆盖嵌入式开发常见场景：
 * - AT 命令模拟（WiFi / 蓝牙）
 * - 二进制 Bootloader 协议
 * - 传感器遥测 / NMEA / CAN 帧输出
 * - Modbus RTU 从机模拟
 * - 交互式调试 Shell
 * - YModem 文件传输握手
 * - IoT 设备初始化序列
 *
 * 首次使用时自动注入到配置列表，按 name 去重。
 */
import type { AutoReplyConfig } from "./types";

let _ruleIdSeq = 0;
/** 生成稳定的内置规则 ID，保证跨会话一致。前缀语义仅供可读性，唯一性由自增计数保证。 */
function randid(prefix: string): string {
  return `${prefix}-${_ruleIdSeq++}`;
}

export const BUILTIN_CONFIGS: AutoReplyConfig[] = [
  // ════════════════════════════════════════════════════════
  // 1. ESP32 AT 命令模拟器
  // ════════════════════════════════════════════════════════
  {
    name: "ESP32 AT 命令模拟器",
    matchStrategy: "first",
    rules: [
      {
        id: randid("esp-at-basic"),
        label: "AT 基础测试",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [{ delayMs: 10, data: "OK\r\n", format: "text" }],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("esp-at-rst"),
        label: "AT+RST 软重启",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+RST\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "OK\r\n", format: "text" },
          { delayMs: 1500, data: "ready\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 5000,
      },
      {
        id: randid("esp-at-gmr"),
        label: "AT+GMR 固件版本",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+GMR\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 10, data: "AT version:2.4.0.0(s-4c6ee32 - Jul 24 2024 18:55:01)\r\n", format: "text" },
          { delayMs: 0, data: "SDK version:v5.3.1\r\n", format: "text" },
          { delayMs: 0, data: "compile time:Jul 24 2024 18:55:01\r\n", format: "text" },
          { delayMs: 0, data: "OK\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("esp-cwmode-q"),
        label: "AT+CWMODE=? 查询范围",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+CWMODE=?\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "+CWMODE:(1-3)\r\n", format: "text" },
          { delayMs: 0, data: "OK\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("esp-cwmode-cur"),
        label: "AT+CWMODE? 查询当前",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+CWMODE?\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "+CWMODE:1\r\n", format: "text" },
          { delayMs: 0, data: "OK\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("esp-cwmode-set"),
        label: "AT+CWMODE=<n> 设置模式",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+CWMODE=", mode: "starts_with", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [{ delayMs: 10, data: "OK\r\n", format: "text" }],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("esp-cwjap"),
        label: "AT+CWJAP? 查询连接 AP",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+CWJAP?\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 10, data: '+CWJAP:"MyWiFi-2G","aa:bb:cc:dd:ee:ff",6,-45\r\n', format: "text" },
          { delayMs: 0, data: "OK\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("esp-cifsr"),
        label: "AT+CIFSR 查询 IP/MAC",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+CIFSR\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 10, data: '+CIFSR:STAIP,"192.168.{{RANDOM(1,254)}}.{{RANDOM(1,254)}}"\r\n', format: "text" },
          { delayMs: 0, data: '+CIFSR:STAMAC,"{{RANDOM(10,99)}}:{{RANDOM(10,99)}}:{{RANDOM(10,99)}}:{{RANDOM(10,99)}}:{{RANDOM(10,99)}}:{{RANDOM(10,99)}}"\r\n', format: "text" },
          { delayMs: 0, data: "OK\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("esp-unknown"),
        label: "未知 AT 命令回退 → ERROR",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+", mode: "starts_with", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [{ delayMs: 10, data: "ERROR\r\n", format: "text" }],
        enabled: true,
        cooldownMs: 0,
      },
    ],
  },

  // ════════════════════════════════════════════════════════
  // 2. STM32 Bootloader 模拟器
  // ════════════════════════════════════════════════════════
  {
    name: "STM32 Bootloader 模拟器",
    matchStrategy: "first",
    rules: [
      {
        id: randid("stm32-sync"),
        label: "同步握手 0x7F → ACK 0x79",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "7F", mode: "equals", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [{ delayMs: 0, data: "79", format: "hex" }],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("stm32-get-cmd"),
        label: "Get 命令列表 (0x00+0xFF)",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "00 FF", mode: "equals", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "79", format: "hex" },
          { delayMs: 0, data: "0B", format: "hex" },
          { delayMs: 0, data: "00 FF 01 FE 02 FD 11 EE 21 DE 31 CE 43 BC 63 9C 73 8C 82 7D 92 6D", format: "hex" },
          { delayMs: 0, data: "79", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("stm32-get-ver"),
        label: "Get Version (0x01+0xFE) → v3.1",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "01 FE", mode: "equals", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "79", format: "hex" },
          { delayMs: 0, data: "02", format: "hex" },
          { delayMs: 0, data: "31 00", format: "hex" },
          { delayMs: 0, data: "79", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("stm32-get-id"),
        label: "Get ID (0x02+0xFD) → PID 0x0413 (STM32F4)",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "02 FD", mode: "equals", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "79", format: "hex" },
          { delayMs: 0, data: "02", format: "hex" },
          { delayMs: 0, data: "04 13", format: "hex" },
          { delayMs: 0, data: "79", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
    ],
  },

  // ════════════════════════════════════════════════════════
  // 3. 传感器遥测模拟器
  // ════════════════════════════════════════════════════════
  {
    name: "传感器遥测模拟器",
    matchStrategy: "all",
    rules: [
      {
        id: randid("sensor-temp-hum"),
        label: "温湿度 JSON (1s)",
        triggerType: "timer",
        timerIntervalMs: 1000,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: '{"t":{{TIMESTAMP}},"temp":{{SIN(20,35,60000)}},"hum":{{RANDOM(40,80)}},"cnt":{{COUNTER}}}\r\n', format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("sensor-power"),
        label: "电源状态 (5s)",
        triggerType: "timer",
        timerIntervalMs: 5000,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "$STAT,{{DATETIME_F(%H:%M:%S)}},U={{SIN(3100,3400,300000)}}mV,I={{RANDOM(10,200)}}mA\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("sensor-lux"),
        label: "环境光 (3s)",
        triggerType: "timer",
        timerIntervalMs: 3000,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "LUX:{{SIN(0,1000,180000)}}\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("sensor-eco2"),
        label: "eCO2 浓度 (10s)",
        triggerType: "timer",
        timerIntervalMs: 10000,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "ECO2:{{RANDOM(400,2000)}} ppm\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("sensor-csv"),
        label: "CSV 日志行 (10s)",
        triggerType: "timer",
        timerIntervalMs: 10000,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "{{TIMESTAMP}},{{COUNTER}},{{SIN(20,35,60000)}},{{SIN(0,1000,180000)}},{{RANDOM(400,2000)}},{{SIN(3100,3400,300000)}},{{RANDOM(10,200)}}\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
    ],
  },

  // ════════════════════════════════════════════════════════
  // 4. 交互式调试 Shell
  // ════════════════════════════════════════════════════════
  {
    name: "MCU 调试 Shell",
    matchStrategy: "first",
    rules: [
      {
        id: randid("shell-help"),
        label: "help — 命令列表",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "help\r\n", mode: "equals", caseSensitive: true, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "Available commands:\r\n", format: "text" },
          { delayMs: 0, data: "  help       Show this message\r\n", format: "text" },
          { delayMs: 0, data: "  version    Firmware version\r\n", format: "text" },
          { delayMs: 0, data: "  status     System status dump\r\n", format: "text" },
          { delayMs: 0, data: "  uptime     System uptime\r\n", format: "text" },
          { delayMs: 0, data: "  rd <addr>  Read 32-bit memory word\r\n", format: "text" },
          { delayMs: 0, data: "  wr <addr> <val> Write 32-bit word\r\n", format: "text" },
          { delayMs: 0, data: "  peek <addr> <n> Hex dump n bytes\r\n", format: "text" },
          { delayMs: 0, data: "$ ", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("shell-version"),
        label: "version — 固件版本",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "version\r\n", mode: "equals", caseSensitive: true, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "TauMCU Debug Shell v2.1.3\r\n", format: "text" },
          { delayMs: 0, data: "Build: {{DATETIME_F(%Y-%m-%d_%H:%M:%S)}}\r\n", format: "text" },
          { delayMs: 0, data: "Board: STM32F407VGT6 @ 168MHz\r\n", format: "text" },
          { delayMs: 0, data: "$ ", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("shell-status"),
        label: "status — 系统状态",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "status\r\n", mode: "equals", caseSensitive: true, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 10, data: "--- System Status ---\r\n", format: "text" },
          { delayMs: 0, data: "Core temp:  {{SIN(35,60,300000)}} C\r\n", format: "text" },
          { delayMs: 0, data: "Stack used: {{RANDOM(2048,8192)}} / 16384 bytes\r\n", format: "text" },
          { delayMs: 0, data: "Heap free:  {{RANDOM(32768,65536)}} bytes\r\n", format: "text" },
          { delayMs: 0, data: "Timestamp:  {{DATETIME}}\r\n", format: "text" },
          { delayMs: 0, data: "$ ", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("shell-uptime"),
        label: "uptime — 运行时间",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "uptime\r\n", mode: "equals", caseSensitive: true, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "System uptime: {{EXPR:COUNTER * 10}} seconds\r\n", format: "text" },
          { delayMs: 0, data: "$ ", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("shell-rd"),
        label: "rd <addr> — 读内存",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "^rd\\s+(0x[0-9a-fA-F]+)\\r\\n$", mode: "regex", caseSensitive: true, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "[{{CAPTURE(1)}}] = 0x{{HEXVAL({{RANDOM(0,4294967295)}},8)}}\r\n", format: "text" },
          { delayMs: 0, data: "$ ", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("shell-wr"),
        label: "wr <addr> <val> — 写内存",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "^wr\\s+(0x[0-9a-fA-F]+)\\s+(0x[0-9a-fA-F]+)\\r\\n$", mode: "regex", caseSensitive: true, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "Wrote {{CAPTURE(2)}} -> [{{CAPTURE(1)}}]\r\n", format: "text" },
          { delayMs: 0, data: "$ ", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("shell-peek"),
        label: "peek <addr> <n> — Hex dump",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "^peek\\s+(0x[0-9a-fA-F]+)\\s+(\\d+)\\r\\n$", mode: "regex", caseSensitive: true, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 10, data: "{{CAPTURE(1)}}: {{HEXVAL({{RANDOM(0,255)}},2)}} {{HEXVAL({{RANDOM(0,255)}},2)}} {{HEXVAL({{RANDOM(0,255)}},2)}} {{HEXVAL({{RANDOM(0,255)}},2)}}  {{HEXVAL({{RANDOM(0,255)}},2)}} {{HEXVAL({{RANDOM(0,255)}},2)}} {{HEXVAL({{RANDOM(0,255)}},2)}} {{HEXVAL({{RANDOM(0,255)}},2)}}\r\n", format: "text" },
          { delayMs: 0, data: "$ ", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("shell-unknown"),
        label: "未知命令回退",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: ".", mode: "regex", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "ERR: unknown command. Type 'help' for list.\r\n", format: "text" },
          { delayMs: 0, data: "$ ", format: "text" },
        ],
        enabled: true,
        cooldownMs: 500,
      },
    ],
  },

  // ════════════════════════════════════════════════════════
  // 5. Modbus RTU 从机模拟器
  // ════════════════════════════════════════════════════════
  {
    name: "Modbus RTU 从机 (ID=1)",
    matchStrategy: "first",
    rules: [
      {
        id: randid("modbus-03"),
        label: "读保持寄存器 (0x03)",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "01 03", mode: "starts_with", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 15, data: "01 03 04 41 C8 00 00 {{CRC(01 03 04 41 C8 00 00, 16, 0x8005)}}", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("modbus-06"),
        label: "写单寄存器 (0x06) — 回显",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "01 06", mode: "starts_with", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 10, data: "01 06 00 01 03 E8 {{CRC(01 06 00 01 03 E8, 16, 0x8005)}}", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("modbus-11"),
        label: "报告从机 ID (0x11)",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "01 11", mode: "starts_with", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 10, data: "01 11 08 54 61 75 54 65 72 6D 20 76 31 {{CRC(01 11 08 54 61 75 54 65 72 6D 20 76 31, 16, 0x8005)}}", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("modbus-04"),
        label: "读输入寄存器 (0x04)",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "01 04", mode: "starts_with", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 15, data: "01 04 04 09 C4 01 F4 {{CRC(01 04 04 09 C4 01 F4, 16, 0x8005)}}", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("modbus-08"),
        label: "诊断 (0x08) — 回显",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "01 08", mode: "starts_with", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 10, data: "01 08 00 00 AA 55 {{CRC(01 08 00 00 AA 55, 16, 0x8005)}}", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("modbus-exc"),
        label: "异常响应 — 非法功能",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "01 2B", mode: "starts_with", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 10, data: "01 AB 01 {{CRC(01 AB 01, 16, 0x8005)}}", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
    ],
  },

  // ════════════════════════════════════════════════════════
  // 6. GPS NMEA 0183 模拟器
  // ════════════════════════════════════════════════════════
  {
    name: "GPS NMEA 0183 模拟器",
    matchStrategy: "all",
    rules: [
      {
        id: randid("nmea-gga"),
        label: "GGA — 定位信息 (1s)",
        triggerType: "timer",
        timerIntervalMs: 1000,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "$GPGGA,{{DATETIME_F(%H%M%S)}}.000,{{RANDOM_F(30,31,6)}},N,{{RANDOM_F(120,121,6)}},E,1,08,1.0,{{SIN(0,500,300000)}},M,0.0,M,,*{{XOR_SUM(GPGGA,{{DATETIME_F(%H%M%S)}}.000,{{RANDOM_F(30,31,6)}},N,{{RANDOM_F(120,121,6)}},E,1,08,1.0,{{SIN(0,500,300000)}},M,0.0,M,,)}}\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("nmea-rmc"),
        label: "RMC — 推荐最小定位 (1s)",
        triggerType: "timer",
        timerIntervalMs: 1000,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "$GPRMC,{{DATETIME_F(%H%M%S)}}.000,A,{{RANDOM_F(30,31,6)}},N,{{RANDOM_F(120,121,6)}},E,{{RANDOM_F(0,5,1)}},{{RANDOM_F(0,360,1)}},{{DATETIME_F(%d%m%y)}},,,A*{{XOR_SUM(GPRMC,{{DATETIME_F(%H%M%S)}}.000,A,{{RANDOM_F(30,31,6)}},N,{{RANDOM_F(120,121,6)}},E,{{RANDOM_F(0,5,1)}},{{RANDOM_F(0,360,1)}},{{DATETIME_F(%d%m%y)}},,,A)}}\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("nmea-gll"),
        label: "GLL — 地理位置 (3s)",
        triggerType: "timer",
        timerIntervalMs: 3000,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "$GPGLL,{{RANDOM_F(30,31,6)}},N,{{RANDOM_F(120,121,6)}},E,{{DATETIME_F(%H%M%S)}}.000,A*{{XOR_SUM(GPGLL,{{RANDOM_F(30,31,6)}},N,{{RANDOM_F(120,121,6)}},E,{{DATETIME_F(%H%M%S)}}.000,A)}}\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("nmea-vtg"),
        label: "VTG — 对地航速 (3s)",
        triggerType: "timer",
        timerIntervalMs: 3000,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "$GPVTG,{{RANDOM_F(0,360,1)}},T,{{SIN(0,15,120000)}},M,{{SIN(0,30,120000)}},N,{{SIN(0,30,120000)}}*{{XOR_SUM(GPVTG,{{RANDOM_F(0,360,1)}},T,{{SIN(0,15,120000)}},M,{{SIN(0,30,120000)}},N,{{SIN(0,30,120000)}})}}\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
    ],
  },

  // ════════════════════════════════════════════════════════
  // 7. YModem 文件传输握手模拟器
  // ════════════════════════════════════════════════════════
  {
    name: "YModem 握手模拟器",
    matchStrategy: "first",
    rules: [
      {
        id: randid("ymodem-C"),
        label: "接收方发送 'C' → 首包 (SOH)",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "43", mode: "equals", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "01 00 FF 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 1000,
      },
      {
        id: randid("ymodem-nak"),
        label: "NAK → 重传上一包",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "15", mode: "equals", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "01 00 FF 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 FF", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 500,
      },
      {
        id: randid("ymodem-ack"),
        label: "ACK → EOT 传输结束",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "06", mode: "equals", caseSensitive: false, negate: false, matchFormat: "hex" }],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "04", format: "hex" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
    ],
  },

  // ════════════════════════════════════════════════════════
  // 8. HC-05 蓝牙 AT 命令模拟器
  // ════════════════════════════════════════════════════════
  {
    name: "HC-05 蓝牙 AT 模拟器",
    matchStrategy: "first",
    rules: [
      {
        id: randid("hc05-at"),
        label: "AT 基础测试",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [{ delayMs: 10, data: "OK\r\n", format: "text" }],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("hc05-ver"),
        label: "AT+VERSION?",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+VERSION?\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "+VERSION:3.0-20190815\r\nOK\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("hc05-name-q"),
        label: "AT+NAME? 查询名称",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+NAME?\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "+NAME:HC-05\r\nOK\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("hc05-name-set"),
        label: "AT+NAME=<name> 设置名称",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+NAME=", mode: "starts_with", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [{ delayMs: 10, data: "OK\r\n", format: "text" }],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("hc05-role"),
        label: "AT+ROLE? 查询角色",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+ROLE?\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "+ROLE:1\r\nOK\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("hc05-addr"),
        label: "AT+ADDR? 查询地址",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT+ADDR?\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "+ADDR:{{RANDOM(1000,9999)}}:{{RANDOM(10,99)}}:{{RANDOM(100000,999999)}}\r\nOK\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("hc05-unknown"),
        label: "未知 AT 命令",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "AT", mode: "starts_with", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [{ delayMs: 10, data: "ERROR\r\n", format: "text" }],
        enabled: true,
        cooldownMs: 0,
      },
    ],
  },

  // ════════════════════════════════════════════════════════
  // 9. IoT 设备初始化序列
  // ════════════════════════════════════════════════════════
  {
    name: "IoT 设备初始化序列",
    matchStrategy: "first",
    rules: [
      {
        id: randid("iot-ready"),
        label: "READY? 查询就绪",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "READY?", mode: "contains", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [{ delayMs: 5, data: "READY\r\n", format: "text" }],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("iot-selftest"),
        label: "SELFTEST 启动自检",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "SELFTEST", mode: "contains", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "START\r\n", format: "text" },
          { delayMs: 500, data: "TEST:RAM...PASS\r\n", format: "text" },
          { delayMs: 200, data: "TEST:FLASH...PASS\r\n", format: "text" },
          { delayMs: 200, data: "TEST:GPIO...PASS\r\n", format: "text" },
          { delayMs: 100, data: "SELFTEST:PASS\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 5000,
      },
      {
        id: randid("iot-fwver"),
        label: "FWVER? 查询固件版本",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "FWVER?\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "FW:v2.1.3-{{DATETIME_F(%Y%m%d)}}\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("iot-devid"),
        label: "DEVID? 查询设备 ID",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "DEVID?\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [
          { delayMs: 5, data: "DEV:{{HEXVAL({{RANDOM(0,65535)}},4)}}-{{HEXVAL({{RANDOM(0,16777215)}},6)}}\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("iot-sleep"),
        label: "SLEEP 进入低功耗",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "SLEEP\r\n", mode: "equals", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [{ delayMs: 10, data: "SLEEP:OK\r\n", format: "text" }],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("iot-wake"),
        label: "WAKE 唤醒设备",
        triggerType: "data",
        timerIntervalMs: 1000,
        conditions: [{ pattern: "WAKE", mode: "contains", caseSensitive: false, negate: false }],
        conditionLogic: "and",
        actions: [{ delayMs: 20, data: "AWAKE\r\n", format: "text" }],
        enabled: true,
        cooldownMs: 0,
      },
    ],
  },

  // ════════════════════════════════════════════════════════
  // 10. CAN 总线帧模拟器
  // ════════════════════════════════════════════════════════
  {
    name: "CAN 总线帧模拟器",
    matchStrategy: "all",
    rules: [
      {
        id: randid("can-rpm"),
        label: "引擎转速 (500ms)",
        triggerType: "timer",
        timerIntervalMs: 500,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "CAN ID:0CF00400 DATA:{{HEXVAL({{SIN(800,6000,10000)}},4)}}0000000000\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("can-speed"),
        label: "车速 (500ms)",
        triggerType: "timer",
        timerIntervalMs: 500,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "CAN ID:18FEF100 DATA:00{{HEXVAL({{SIN(0,120,30000)}},2)}}0000000000\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("can-batt"),
        label: "电池电压 (1s)",
        triggerType: "timer",
        timerIntervalMs: 1000,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "CAN ID:18FFA327 DATA:{{HEXVAL({{SIN(11000,14800,60000)}},4)}}0000000000\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
      {
        id: randid("can-dtc"),
        label: "故障码 (5s)",
        triggerType: "timer",
        timerIntervalMs: 5000,
        conditions: [],
        conditionLogic: "and",
        actions: [
          { delayMs: 0, data: "CAN ID:18FECA00 DATA:{{HEXVAL({{RANDOM(0,255)}},2)}}{{HEXVAL({{RANDOM(0,255)}},2)}}0000000000\r\n", format: "text" },
        ],
        enabled: true,
        cooldownMs: 0,
      },
    ],
  },
];
