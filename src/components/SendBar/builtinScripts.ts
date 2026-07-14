/**
 * 内置 Lua 脚本示例。
 *
 * 10 个脚本覆盖嵌入式开发常见场景，从入门到高级，
 * 系统性地演示所有可用 API（send / sleep / log / on_data /
 * register_timer / unregister_timer / regex_find / _time_ms /
 * _datetime_iso / _datetime_format）。
 *
 * 首次使用时自动注入到脚本列表，按 id 去重。
 */
import type { ScriptRecord } from "./types";

export const BUILTIN_SCRIPTS: ScriptRecord[] = [
  // ════════════════════════════════════════════════════════
  // 1. Hello Serial — 入门
  // ════════════════════════════════════════════════════════
  {
    id: "builtin-script-01",
    name: "Hello Serial",
    code: `-- Hello Serial
-- 向串口发送欢迎横幅并记录日志，演示最基础的 send / log / sleep API。

send("\\r\\n========================================\\r\\n")
log("已发送顶部边框")

send("||        Hello from TauTerm!          ||\\r\\n")
log("已发送欢迎消息")

send("||   Embedded Lua Scripting Engine    ||\\r\\n")
log("已发送功能描述")

send("========================================\\r\\n\\r\\n")
log("横幅发送完毕 — 脚本结束")
`,
    createdAt: 0,
    updatedAt: 0,
  },

  // ════════════════════════════════════════════════════════
  // 2. 串口回显服务器 — 入门
  // ════════════════════════════════════════════════════════
  {
    id: "builtin-script-02",
    name: "串口回显服务器",
    code: `-- 串口回显服务器
-- 将所有收到的数据加上 [ECHO] 前缀后原样回传。
-- 演示 on_data 全匹配模式。

log("回显服务器已启动 — 等待数据...")

-- 使用 ".-" 匹配任意数据（表示"捕获0个或多个任意字符"）
on_data("%.%-", function(data)
  send("[ECHO] " .. data)
  log("回显: " .. data)
end)
`,
    createdAt: 0,
    updatedAt: 0,
  },

  // ════════════════════════════════════════════════════════
  // 3. 定时命令序列 — 入门
  // ════════════════════════════════════════════════════════
  {
    id: "builtin-script-03",
    name: "定时命令序列",
    code: `-- 定时命令序列
-- 按固定间隔依次发送 AT 命令，模拟设备查询流程。
-- 演示 send / sleep / log 的编排使用。

local commands = {
  { cmd = "AT\\r\\n",           desc = "基础连接测试" },
  { cmd = "AT+CSQ\\r\\n",       desc = "查询信号质量" },
  { cmd = "AT+CREG?\\r\\n",     desc = "查询网络注册状态" },
  { cmd = "AT+CGATT?\\r\\n",    desc = "查询 GPRS 附着状态" },
}

log("开始执行定时命令序列 (" .. #commands .. " 条)")

for i, item in ipairs(commands) do
  send(item.cmd)
  log(string.format("[%d/%d] 已发送: %s", i, #commands, item.desc))
  sleep(800)  -- 等待设备响应
end

log("命令序列执行完毕")
`,
    createdAt: 0,
    updatedAt: 0,
  },

  // ════════════════════════════════════════════════════════
  // 4. ESP32 AT 命令响应 — 初级
  // ════════════════════════════════════════════════════════
  {
    id: "builtin-script-04",
    name: "ESP32 AT 命令响应",
    code: `-- ESP32 AT 命令响应
-- 模拟 ESP32 对常见 AT 命令的响应。
-- 演示 on_data 模式匹配（Lua string.find 模式）。

log("ESP32 AT 响应脚本已启动")

-- 匹配 "AT\\r\\n"（基本 AT 测试）
on_data("AT\\r\\n", function(data)
  send("\\r\\nOK\\r\\n")
  log("响应: AT -> OK")
end)

-- 匹配 "AT+GMR\\r\\n"（查询固件版本）
on_data("AT%+GMR\\r\\n", function(data)
  send("\\r\\nAT version:2.2.0.0(s-d41b70e)")
  send("\\r\\nSDK version:v4.4.7")
  send("\\r\\ncompile time:Jun 15 2024 10:30:00")
  send("\\r\\nOK\\r\\n")
  log("响应: AT+GMR -> 版本信息")
end)

-- 匹配 "AT+CWMODE?"（查询 WiFi 模式）
on_data("AT%+CWMODE%?\\r\\n", function(data)
  send("\\r\\n+CWMODE:1\\r\\n\\r\\nOK\\r\\n")
  log("响应: AT+CWMODE? -> Station 模式")
end)
`,
    createdAt: 0,
    updatedAt: 0,
  },

  // ════════════════════════════════════════════════════════
  // 5. 传感器数据日志器 — 中级
  // ════════════════════════════════════════════════════════
  {
    id: "builtin-script-05",
    name: "传感器数据日志器",
    code: `-- 传感器数据日志器
-- 每 5 秒生成一条带时间戳的 CSV 传感器数据行。
-- 演示 register_timer / _datetime_iso / send / log。

log("传感器日志器已启动 — 每 5 秒记录一次")

local count = 0

register_timer("sensor-log", 5000, function()
  count = count + 1
  local ts = _datetime_iso()
  -- 模拟传感器读数（实际使用时替换为真实数据解析）
  local temp   = string.format("%.1f", 25.0 + math.sin(count * 0.5) * 5)
  local humid  = string.format("%.1f", 55.0 + math.cos(count * 0.3) * 10)
  local volt   = string.format("%.2f", 3.30 + math.sin(count * 0.7) * 0.05)
  local line   = ts .. "," .. temp .. "," .. humid .. "," .. volt .. "\\r\\n"
  send(line)
  log(string.format("[%d] %s", count, line))
end)
`,
    createdAt: 0,
    updatedAt: 0,
  },

  // ════════════════════════════════════════════════════════
  // 6. Modbus RTU 寄存器轮询 — 中级
  // ════════════════════════════════════════════════════════
  {
    id: "builtin-script-06",
    name: "Modbus RTU 寄存器轮询",
    code: `-- Modbus RTU 寄存器轮询
-- 每秒发送一次 Modbus RTU 读保持寄存器请求，并解析响应。
-- 演示 register_timer / on_data / send / log / _time_ms / 二进制数据构造。
--
-- 请求帧: 01 03 00 00 00 02 C4 0B
--   01       = 从机地址 1
--   03       = 功能码（读保持寄存器）
--   00 00    = 起始地址 0
--   00 02    = 寄存器数量 2
--   C4 0B    = CRC-16
-- 响应帧: 01 03 04 xx xx xx xx crc crc

log("Modbus RTU 轮询器已启动 — 每秒查询一次从机 #1")

-- 构造 Modbus 请求帧
local function make_read_holding(addr, start_reg, count)
  -- 简化的 CRC-16 计算
  local function crc16(data)
    local crc = 0xFFFF
    for i = 1, #data do
      crc = crc ~ data:byte(i)
      for _ = 1, 8 do
        local lsb = crc & 1
        crc = crc >> 1
        if lsb == 1 then crc = crc ~ 0xA001 end
      end
    end
    return crc
  end

  local frame = string.char(addr, 3,
    (start_reg >> 8) & 0xFF, start_reg & 0xFF,
    (count >> 8) & 0xFF, count & 0xFF)
  local crc = crc16(frame)
  return frame .. string.char(crc & 0xFF, (crc >> 8) & 0xFF)
end

-- 定时发送请求
register_timer("modbus-poll", 1000, function()
  local req = make_read_holding(1, 0, 2)
  send(req)
  log("已发送 Modbus 请求 (从机 #1, 寄存器 0-1)")
end)

-- 监听响应
on_data(".-", function(data)
  -- 检查是否为有效 Modbus 响应（至少 5 字节: addr + func + len + data + crc）
  if #data >= 5 then
    local addr = data:byte(1)
    local func = data:byte(2)
    if func == 3 then
      local byte_count = data:byte(3)
      local regs = {}
      for i = 0, (byte_count / 2) - 1 do
        local hi = data:byte(4 + i * 2)
        local lo = data:byte(5 + i * 2)
        regs[#regs + 1] = (hi << 8) | lo
      end
      local ts = _time_ms()
      log(string.format("[%d] 响应: 从机=%d, 寄存器=%s",
        ts, addr, table.concat(regs, ", ")))
    end
  end
end)
`,
    createdAt: 0,
    updatedAt: 0,
  },

  // ════════════════════════════════════════════════════════
  // 7. GPS NMEA 0183 解析器 — 中级
  // ════════════════════════════════════════════════════════
  {
    id: "builtin-script-07",
    name: "GPS NMEA 0183 解析器",
    code: `-- GPS NMEA 0183 解析器
-- 解析 GGA 和 RMC 语句，提取位置、速度、高度等信息。
-- 演示 on_data 模式匹配 + regex_find + log。

log("GPS NMEA 解析器已启动 — 等待 NMEA 语句...")

-- GGA 语句正则: $GPGGA,time,lat,N,lon,E,quality,sats,hdop,alt,M,...
local GGA_RE = [[\\$GPGGA,(\\d+\\.?\\d*),([\\d.]+),([NS]),([\\d.]+),([EW]),(\\d),(\\d+),([\\d.]+),([\\d.]+),M,]]

-- RMC 语句正则: $GPRMC,time,status,lat,N,lon,E,speed,track,date,...
local RMC_RE = [[\\$GPRMC,(\\d+\\.?\\d*),(\\w),([\\d.]+),([NS]),([\\d.]+),([EW]),([\\d.]+),([\\d.]+),(\\d+),]]

on_data("$GP.-", function(data)
  -- 尝试匹配 GGA
  local caps = regex_find(GGA_RE, data)
  if caps then
    local time    = caps[1]
    local lat     = caps[2] .. caps[3]
    local lon     = caps[4] .. caps[5]
    local quality = tonumber(caps[6])
    local sats    = caps[7]
    local alt     = caps[9]
    local qlabel  = ({ [0]="无效", [1]="单点", [2]="差分" })[quality] or "未知"
    log(string.format("GGA | 时间:%s 纬度:%s 经度:%s 质量:%s 卫星:%s 高度:%sm",
      time, lat, lon, qlabel, sats, alt))
    return  -- 已处理 GGA
  end

  -- 尝试匹配 RMC
  caps = regex_find(RMC_RE, data)
  if caps then
    local time   = caps[1]
    local status = caps[2] == "A" and "有效" or "无效"
    local lat    = caps[3] .. caps[4]
    local lon    = caps[5] .. caps[6]
    local speed  = caps[7]
    local track  = caps[8]
    log(string.format("RMC | 时间:%s 状态:%s 纬度:%s 经度:%s 速度:%skn 航向:%s°",
      time, status, lat, lon, speed, track))
    return
  end

  -- 未识别的 NMEA 语句
  log("NMEA: " .. data:gsub("\\r\\n", ""))
end)
`,
    createdAt: 0,
    updatedAt: 0,
  },

  // ════════════════════════════════════════════════════════
  // 8. 正则数据提取器 — 高级
  // ════════════════════════════════════════════════════════
  {
    id: "builtin-script-08",
    name: "正则数据提取器",
    code: `-- 正则数据提取器
-- 从设备遥测行中提取温度/湿度/气压，超阈值时告警。
-- 演示 on_data + regex_find 多捕获组 + 条件逻辑。
--
-- 输入格式: "TEMP:36.7,HUM:65.2,PRESS:1013.2"
-- 也支持:     "T=25.4 H=80.1 P=990.5"

log("数据提取器已启动 — 阈值告警: TEMP>38, HUM>75, PRESS<1000")

local ALERT_TEMP  = 38.0
local ALERT_HUM   = 75.0
local ALERT_PRESS = 1000.0

-- 格式 1: "TEMP:xx,HUM:xx,PRESS:xx"
local FMT1 = [[TEMP:(\\d+\\.?\\d*),HUM:(\\d+\\.?\\d*),PRESS:(\\d+\\.?\\d*)]]

-- 格式 2: "T=xx H=xx P=xx"
local FMT2 = [[T=(\\d+\\.?\\d*) H=(\\d+\\.?\\d*) P=(\\d+\\.?\\d*)]]

on_data(".-", function(data)
  local temp, hum, press = nil, nil, nil

  local caps = regex_find(FMT1, data)
  if caps then
    temp  = tonumber(caps[1])
    hum   = tonumber(caps[2])
    press = tonumber(caps[3])
  else
    caps = regex_find(FMT2, data)
    if caps then
      temp  = tonumber(caps[1])
      hum   = tonumber(caps[2])
      press = tonumber(caps[3])
    end
  end

  if temp then
    log(string.format("传感器 | 温度:%.1f°C  湿度:%.1f%%  气压:%.1fhPa", temp, hum, press))

    local alerts = {}
    if temp  > ALERT_TEMP  then alerts[#alerts + 1] = "高温告警!" end
    if hum   > ALERT_HUM   then alerts[#alerts + 1] = "高湿告警!" end
    if press < ALERT_PRESS then alerts[#alerts + 1] = "低气压告警!" end

    for _, a in ipairs(alerts) do
      log("[告警] " .. a)
      send("ALERT:" .. a .. "\\r\\n")
    end
  end
end)
`,
    createdAt: 0,
    updatedAt: 0,
  },

  // ════════════════════════════════════════════════════════
  // 9. MCU Bootloader 协议 — 高级
  // ════════════════════════════════════════════════════════
  {
    id: "builtin-script-09",
    name: "MCU Bootloader 协议",
    code: `-- MCU Bootloader 协议
-- 实现 STM32 内置 Bootloader（USART 协议）握手流程。
-- 演示 send / sleep / on_data + 多步协议状态机。
--
-- 协议:
--   1. 发送 0x7F（同步字节）
--   2. 等待 ACK (0x79)
--   3. 发送 0x01 0xFE（Get Version 命令 + 校验）
--   4. 解析版本响应

log("Bootloader 协议脚本已启动")

-- 状态机
local READY  = false
local bootloader_version = nil

-- 同步命令
send(string.char(0x7F))
log("已发送同步字节 0x7F")

-- 启动超时定时器（3 秒内无响应则取消）
local boot_timeout = false
register_timer("boot-timeout", 3000, function()
  if not READY then
    log("[超时] Bootloader 无响应 — 设备可能不在 Bootloader 模式")
    boot_timeout = true
  end
  unregister_timer("boot-timeout")
end)

-- 监听响应
on_data(".-", function(data)
  if boot_timeout then return end

  -- 检查 ACK (0x79)
  if data:byte(1) == 0x79 and not READY then
    READY = true
    unregister_timer("boot-timeout")
    log("收到 ACK — 进入 Bootloader 命令模式")
    -- 发送 Get Version 命令
    sleep(50)
    send(string.char(0x01, 0xFE))
    log("已发送 Get Version 命令 (0x01 0xFE)")
    return
  end

  -- 解析版本响应: ACK (0x79) + version (1 byte) + opt1 + opt2 + ACK (0x79)
  if data:byte(1) == 0x79 and READY and #data >= 3 and not bootloader_version then
    local version = data:byte(2)
    local opt1    = data:byte(3)
    local opt2    = (#data >= 4) and data:byte(4) or 0
    bootloader_version = string.format("v%d.%d (opts: 0x%02X 0x%02X)", version, 0, opt1, opt2)
    log("Bootloader 版本: " .. bootloader_version)
    log("握手完成 — 设备就绪")
  end
end)

log("等待 Bootloader 响应...")
`,
    createdAt: 0,
    updatedAt: 0,
  },

  // ════════════════════════════════════════════════════════
  // 10. IoT 设备初始化向导 — 高级
  // ════════════════════════════════════════════════════════
  {
    id: "builtin-script-10",
    name: "IoT 设备初始化向导",
    code: `-- IoT 设备初始化向导
-- 执行多阶段设备初始化: 就绪检查 -> 自检 -> 固件查询 -> 启动。
-- 演示 on_data / send / sleep / log + Lua 表状态机。
--
-- 协议: 纯文本命令行风格
--   PC -> DEV: "READY?"
--   DEV -> PC: "READY\\r\\n"
--   PC -> DEV: "SELFTEST"
--   DEV -> PC: "SELFTEST:PASS\\r\\n" 或 "SELFTEST:FAIL\\r\\n"
--   PC -> DEV: "FWVER?"
--   DEV -> PC: "FWVER:1.2.3\\r\\n"
--   PC -> DEV: "START"
--   DEV -> PC: "RUNNING\\r\\n"

log("IoT 设备初始化向导已启动")

-- 状态机: stage -> { cmd, expected, timeout_ms }
local STAGE_READY    = 1
local STAGE_SELFTEST = 2
local STAGE_FWVER    = 3
local STAGE_START    = 4
local STAGE_DONE     = 5

local stage     = STAGE_READY
local stage_ok  = false
local MAX_WAIT  = 5000  -- 每阶段最长等待 5 秒

-- 发送当前阶段命令
local function run_stage(next_stage)
  stage = next_stage
  stage_ok = false

  if next_stage == STAGE_READY then
    send("READY?\\r\\n")
    log("[阶段1] 已发送: READY?")
  elseif next_stage == STAGE_SELFTEST then
    send("SELFTEST\\r\\n")
    log("[阶段2] 已发送: SELFTEST")
  elseif next_stage == STAGE_FWVER then
    send("FWVER?\\r\\n")
    log("[阶段3] 已发送: FWVER?")
  elseif next_stage == STAGE_START then
    send("START\\r\\n")
    log("[阶段4] 已发送: START")
  end
end

-- 各阶段超时定时器
local stage_timeout_id = "stage-timeout"
register_timer(stage_timeout_id, MAX_WAIT, function()
  if stage == STAGE_DONE then
    unregister_timer(stage_timeout_id)
    return
  end
  log(string.format("[超时] 阶段 %d 无响应 — 初始化失败", stage))
  unregister_timer(stage_timeout_id)
end)

-- 监听设备响应
on_data(".-", function(data)
  if stage == STAGE_DONE then return end

  if stage == STAGE_READY and data:find("READY") then
    stage_ok = true
    log("[阶段1] 设备就绪确认")
    sleep(200)
    run_stage(STAGE_SELFTEST)

  elseif stage == STAGE_SELFTEST and data:find("SELFTEST") then
    if data:find("PASS") then
      stage_ok = true
      log("[阶段2] 自检通过")
      sleep(200)
      run_stage(STAGE_FWVER)
    elseif data:find("FAIL") then
      log("[阶段2] 自检失败 — 初始化终止")
      stage = STAGE_DONE
      unregister_timer(stage_timeout_id)
    end

  elseif stage == STAGE_FWVER and data:find("FWVER:") then
    local ver = data:match("FWVER:(%S+)")
    log("[阶段3] 固件版本: " .. (ver or "未知"))
    stage_ok = true
    sleep(200)
    run_stage(STAGE_START)

  elseif stage == STAGE_START and data:find("RUNNING") then
    stage = STAGE_DONE
    stage_ok = true
    unregister_timer(stage_timeout_id)
    log("[阶段4] 设备已启动运行")
    log("初始化完成 — 全部 4 阶段通过")
  end
end)

-- 启动流程
run_stage(STAGE_READY)
`,
    createdAt: 0,
    updatedAt: 0,
  },
];
