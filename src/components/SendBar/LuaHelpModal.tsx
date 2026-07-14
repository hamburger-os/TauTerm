import { useState, useCallback, useEffect } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import styles from "./LuaHelpModal.module.css";

interface LuaHelpModalProps {
  isOpen: boolean;
  onClose: () => void;
}

// ── 帮助内容类型 ──────────────────────────────────────

interface FunctionEntry {
  name: string;
  signature: string;
  desc: string;
  example?: string;
}

interface MacroEntry {
  name: string;
  syntax: string;
  desc: string;
  example?: string;
}

type HelpCategory = "functions" | "macros" | "sandbox";

interface HelpContent {
  functions: FunctionEntry[];
  macros: MacroEntry[];
  sandbox: {
    title: string;
    intro: string;
    restrictions: string[];
    notes: string[];
  };
}

// ── 帮助内容（中文）───────────────────────────────────

const helpContent: HelpContent = {
  functions: [
    {
      name: "send",
      signature: "send(data: string)",
      desc: "通过当前连接的串口发送原始字节数据。data 可以是任意字符串（含 \\0 等二进制字节）。",
      example: `send("AT\\r\\n")\nsend("\\x01\\x03\\x00\\x00\\x00\\x0A")`,
    },
    {
      name: "sleep",
      signature: "sleep(ms: number)",
      desc: "暂停脚本执行指定毫秒数。采用协作式分片睡眠（50ms 片），停止脚本时可及时中断。",
      example: `sleep(1000)  -- 等待 1 秒`,
    },
    {
      name: "log",
      signature: "log(message: string)",
      desc: "向 Script Output 面板输出日志消息。自动附加时间戳（格式 HH:MM:SS.mmm）。",
      example: `log("设备响应: " .. data)`,
    },
    {
      name: "on_data",
      signature: "on_data(pattern: string, callback: function)",
      desc: "注册数据接收回调。当收到的数据包含指定 pattern（Lua 模式匹配）时，自动调用 callback(data)。可多次调用注册多个处理器。",
      example: `on_data("OK", function(data)\n    log("收到OK: " .. data)\nend)`,
    },
    {
      name: "register_timer",
      signature: "register_timer(id: string, interval_ms: number, callback: function)",
      desc: "注册周期性定时器。每隔 interval_ms 毫秒触发一次 callback。首次触发为立即执行（last_fire 初始为 0）。",
      example: `register_timer("poll", 5000, function()\n    send("AT+CSQ\\r\\n")\nend)`,
    },
    {
      name: "unregister_timer",
      signature: "unregister_timer(id: string)",
      desc: "停止并移除指定 id 的定时器。",
      example: `unregister_timer("poll")`,
    },
    {
      name: "regex_find",
      signature: "regex_find(pattern: string, data: string) → table | nil",
      desc: "使用 Rust regex 引擎进行正则匹配。返回捕获组表（0-indexed：[0] = 完整匹配，[1] = 第一组…），无匹配返回 nil。支持完整正则语法（\\d+、\\s*、[a-z]+ 等）。",
      example: `local caps = regex_find("TEMP:(%d+%.?%d*)", data)\nif caps then\n    log("温度: " .. caps[1] .. "°C")\nend`,
    },
    {
      name: "_time_ms",
      signature: "_time_ms() → number",
      desc: "返回当前 Unix 毫秒时间戳。",
      example: `local ts = _time_ms()\nlog("当前时间戳: " .. ts)`,
    },
    {
      name: "_datetime_iso",
      signature: "_datetime_iso() → string",
      desc: "返回当前本地时间的 ISO 8601 格式字符串（YYYY-MM-DDTHH:MM:SS）。",
      example: `log("当前时间: " .. _datetime_iso())`,
    },
    {
      name: "_datetime_format",
      signature: "_datetime_format(format: string) → string",
      desc: "返回自定义 strftime 格式的日期时间字符串。支持 %Y %m %d %H %M %S 等标准格式说明符。",
      example: `log(_datetime_format("%Y-%m-%d %H:%M:%S"))`,
    },
  ],

  macros: [
    {
      name: "CAPTURE",
      syntax: "{{CAPTURE(n)}}",
      desc: "正则捕获组引用。n 为捕获组索引（1-indexed）。仅在正则匹配模式下可用。",
      example: "正则 ^TEMP:(\\d+\\.\\d+)\n回复 {{CAPTURE(1)}} °C",
    },
    {
      name: "RANDOM",
      syntax: "{{RANDOM(min, max)}}",
      desc: "生成 [min, max] 范围内的随机整数。",
      example: "{{RANDOM(0, 255)}} → 183",
    },
    {
      name: "RANDOM_F",
      syntax: "{{RANDOM_F(min, max, decimals)}}",
      desc: "生成 [min, max] 范围内的随机浮点数，保留指定小数位数。",
      example: "{{RANDOM_F(20.0, 30.0, 1)}} → 25.7",
    },
    {
      name: "TIMESTAMP",
      syntax: "{{TIMESTAMP}}",
      desc: "当前 Unix 毫秒时间戳。",
      example: "{{TIMESTAMP}} → 1720934400000",
    },
    {
      name: "DATETIME",
      syntax: "{{DATETIME}}",
      desc: "当前本地日期时间，ISO 8601 格式。",
      example: "{{DATETIME}} → 2026-07-14T13:30:00",
    },
    {
      name: "DATETIME_F",
      syntax: "{{DATETIME_F(format)}}",
      desc: "自定义 strftime 格式的日期时间。",
      example: "{{DATETIME_F(%H:%M:%S)}} → 13:30:00",
    },
    {
      name: "COUNTER",
      syntax: "{{COUNTER}}",
      desc: "规则级自增计数器。每次匹配递增 1，从 1 开始。",
      example: "{{COUNTER}} → 1 (首次), 2 (二次)...",
    },
    {
      name: "HEX",
      syntax: "{{HEX(text)}}",
      desc: "将文本字符串转换为大写 HEX 字符串（每字节两位十六进制）。",
      example: "{{HEX(OK)}} → 4F4B",
    },
    {
      name: "HEXVAL",
      syntax: "{{HEXVAL(num, width)}}",
      desc: "将数字格式化为固定宽度的大写 HEX 字符串。",
      example: "{{HEXVAL(255, 2)}} → FF",
    },
    {
      name: "SIN",
      syntax: "{{SIN(min, max, period_ms)}}",
      desc: "生成正弦波传感器模拟值，在 [min, max] 范围内以 period_ms 为周期变化。",
      example: "{{SIN(0, 100, 60000)}} → 每分钟 0→100→0",
    },
    {
      name: "CRC",
      syntax: "{{CRC(data, width, poly)}}",
      desc: "统一 CRC 计算引擎。width: 8/16/32。内置预置：CRC-8(0x07)、CRC-16/Modbus(0x8005)、CRC-16/CCITT(0x1021)、CRC-32(0x04C11DB7)。返回大写 HEX。",
      example: "{{CRC(hello, 16, 0x8005)}} → Modbus CRC-16",
    },
    {
      name: "XOR_SUM",
      syntax: "{{XOR_SUM(data)}}",
      desc: "逐字节 XOR 校验和（NMEA *XX 格式）。返回 2 位大写 HEX。",
      example: "{{XOR_SUM(GPGGA,...)}} → 5C",
    },
    {
      name: "SUM8",
      syntax: "{{SUM8(data)}}",
      desc: "逐字节求和取低 8 位（Intel HEX 校验格式）。返回 2 位大写 HEX。",
      example: "{{SUM8(:10000000...)}} → A3",
    },
    {
      name: "EXPR",
      syntax: "{{EXPR:expression}}",
      desc: "安全算术表达式求值。支持 + - * / % ^ 及位运算 & | ~ << >>。表达式经字符白名单校验，仅允许数字、运算符和括号。",
      example: "{{EXPR:2 + 3 * 4}} → 14",
    },
  ],

  sandbox: {
    title: "安全限制",
    intro: "每个会话的 Lua VM 运行在独立的 std::thread 中，采用以下安全措施确保脚本不会影响系统稳定性或访问敏感资源：",
    restrictions: [
      "os 模块已移除 — 无法执行系统命令或访问环境变量",
      "io 模块已移除 — 无法读写文件系统",
      "require 已移除 — 无法加载外部 C 扩展模块",
      "dofile / loadfile 已移除 — 无法从磁盘加载 Lua 文件",
      "debug 已移除 — 无法操作元表或自省 VM 内部结构",
      "内存限制 1MB — 防止恶意或错误的无限分配",
    ],
    notes: [
      "load 函数保留 — 代码生成器的 EXPR 宏需要 load() 进行算术求值，生成的表达式经字符白名单校验，仅允许数字、运算符和括号，风险可控",
      "保留的安全模块：string, table, math, coroutine（纯计算，无 I/O 能力）",
      "脚本在独立线程运行，崩溃或 panic 不影响主进程或 I/O 循环",
      "send() 通过 CommHandle 抽象层路由到 I/O 线程，不直接操作串口句柄",
    ],
  },
};

// ── 英文翻译映射 ────────────────────────────────────

const enContent: HelpContent = {
  functions: [
    { name: "send", signature: "send(data: string)", desc: "Send raw bytes through the connected serial port. data can contain any bytes including \\0.", example: `send("AT\\r\\n")\nsend("\\x01\\x03\\x00\\x00\\x00\\x0A")` },
    { name: "sleep", signature: "sleep(ms: number)", desc: "Pause script execution for ms milliseconds. Uses cooperative 50ms-slice sleep for responsive shutdown.", example: `sleep(1000)  -- wait 1 second` },
    { name: "log", signature: "log(message: string)", desc: "Output a log message to the Script Output panel. Timestamp auto-prepended (HH:MM:SS.mmm).", example: `log("Device response: " .. data)` },
    { name: "on_data", signature: "on_data(pattern: string, callback: function)", desc: "Register a data receive handler. When received data contains pattern (Lua pattern match), callback(data) is invoked. Call multiple times for multiple handlers.", example: `on_data("OK", function(data)\n    log("Got OK: " .. data)\nend)` },
    { name: "register_timer", signature: "register_timer(id: string, interval_ms: number, callback: function)", desc: "Register a periodic timer. Fires callback every interval_ms ms. First fire is immediate (last_fire starts at 0).", example: `register_timer("poll", 5000, function()\n    send("AT+CSQ\\r\\n")\nend)` },
    { name: "unregister_timer", signature: "unregister_timer(id: string)", desc: "Stop and remove a timer by its id.", example: `unregister_timer("poll")` },
    { name: "regex_find", signature: "regex_find(pattern: string, data: string) → table | nil", desc: "Full regex matching via Rust regex engine. Returns capture group table (0-indexed: [0]=full match, [1]=group 1…), nil if no match.", example: `local caps = regex_find("TEMP:(%d+%.?%d*)", data)\nif caps then\n    log("Temp: " .. caps[1] .. "°C")\nend` },
    { name: "_time_ms", signature: "_time_ms() → number", desc: "Returns current Unix timestamp in milliseconds.", example: `local ts = _time_ms()\nlog("Timestamp: " .. ts)` },
    { name: "_datetime_iso", signature: "_datetime_iso() → string", desc: "Returns current local time as ISO 8601 string (YYYY-MM-DDTHH:MM:SS).", example: `log("Time: " .. _datetime_iso())` },
    { name: "_datetime_format", signature: "_datetime_format(format: string) → string", desc: "Returns custom strftime-formatted datetime string. Supports %Y %m %d %H %M %S etc.", example: `log(_datetime_format("%Y-%m-%d %H:%M:%S"))` },
  ],
  macros: [
    { name: "CAPTURE", syntax: "{{CAPTURE(n)}}", desc: "Regex capture group reference. n is the 1-indexed group number. Only available in regex match mode.", example: "Pattern ^TEMP:(\\d+\\.\\d+)\nReply {{CAPTURE(1)}} °C" },
    { name: "RANDOM", syntax: "{{RANDOM(min, max)}}", desc: "Generate random integer in [min, max].", example: "{{RANDOM(0, 255)}} → 183" },
    { name: "RANDOM_F", syntax: "{{RANDOM_F(min, max, decimals)}}", desc: "Generate random float in [min, max] with specified decimal places.", example: "{{RANDOM_F(20.0, 30.0, 1)}} → 25.7" },
    { name: "TIMESTAMP", syntax: "{{TIMESTAMP}}", desc: "Current Unix millisecond timestamp.", example: "{{TIMESTAMP}} → 1720934400000" },
    { name: "DATETIME", syntax: "{{DATETIME}}", desc: "Current local datetime in ISO 8601 format.", example: "{{DATETIME}} → 2026-07-14T13:30:00" },
    { name: "DATETIME_F", syntax: "{{DATETIME_F(format)}}", desc: "Custom strftime-formatted datetime.", example: "{{DATETIME_F(%H:%M:%S)}} → 13:30:00" },
    { name: "COUNTER", syntax: "{{COUNTER}}", desc: "Rule-level auto-increment counter. Starts at 1.", example: "{{COUNTER}} → 1, 2, 3..." },
    { name: "HEX", syntax: "{{HEX(text)}}", desc: "Convert text to uppercase HEX string.", example: "{{HEX(OK)}} → 4F4B" },
    { name: "HEXVAL", syntax: "{{HEXVAL(num, width)}}", desc: "Format number as fixed-width uppercase HEX.", example: "{{HEXVAL(255, 2)}} → FF" },
    { name: "SIN", syntax: "{{SIN(min, max, period_ms)}}", desc: "Sine wave sensor simulator varying in [min, max] over period_ms.", example: "{{SIN(0, 100, 60000)}} → 0→100→0 per minute" },
    { name: "CRC", syntax: "{{CRC(data, width, poly)}}", desc: "Unified CRC engine. width: 8/16/32. Built-in presets: CRC-8(0x07), CRC-16/Modbus(0x8005), CRC-16/CCITT(0x1021), CRC-32(0x04C11DB7).", example: "{{CRC(hello, 16, 0x8005)}} → Modbus CRC-16" },
    { name: "XOR_SUM", syntax: "{{XOR_SUM(data)}}", desc: "Byte-wise XOR checksum (NMEA *XX format). Returns 2-char uppercase HEX.", example: "{{XOR_SUM(GPGGA,...)}} → 5C" },
    { name: "SUM8", syntax: "{{SUM8(data)}}", desc: "Byte-wise sum modulo 256 (Intel HEX checksum). Returns 2-char uppercase HEX.", example: "{{SUM8(:10000000...)}} → A3" },
    { name: "EXPR", syntax: "{{EXPR:expression}}", desc: "Safe arithmetic expression evaluator. Supports + - * / % ^ and bitwise & | ~ << >>. Character-whitelisted.", example: "{{EXPR:2 + 3 * 4}} → 14" },
  ],
  sandbox: {
    title: "Security Restrictions",
    intro: "Each session's Lua VM runs in an independent std::thread with the following safeguards:",
    restrictions: [
      "os module removed — cannot execute system commands or access environment variables",
      "io module removed — cannot read/write filesystem",
      "require removed — cannot load external C extension modules",
      "dofile / loadfile removed — cannot load Lua files from disk",
      "debug removed — cannot manipulate metatables or introspect VM internals",
      "1MB memory limit — prevents malicious or accidental unbounded allocation",
    ],
    notes: [
      "load function preserved — the EXPR macro in codegen uses load() for arithmetic evaluation; generated expressions pass a character whitelist (digits, operators, parens only), risk is controlled",
      "Preserved safe modules: string, table, math, coroutine (pure computation, no I/O)",
      "Scripts run in isolated threads; crashes or panics do not affect the main process or I/O loop",
      "send() routes through the CommHandle abstraction layer to the I/O thread, never accesses serial port handles directly",
    ],
  },
};

const CATEGORIES: { id: HelpCategory; icon: import("../common/Icon").IconName; labelKey: string }[] = [
  { id: "functions", icon: "code" as const, labelKey: "sendBar.helpFunctions" },
  { id: "macros", icon: "robot" as const, labelKey: "sendBar.helpMacros" },
  { id: "sandbox", icon: "info" as const, labelKey: "sendBar.helpSandbox" },
];

export default function LuaHelpModal({ isOpen, onClose }: LuaHelpModalProps) {
  const { t, i18n } = useTranslation();
  const [activeCategory, setActiveCategory] = useState<HelpCategory>("functions");

  // Esc 关闭
  useEffect(() => {
    if (!isOpen) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [isOpen, onClose]);

  // 重置分类
  useEffect(() => {
    if (isOpen) setActiveCategory("functions");
  }, [isOpen]);

  const handleOverlayClick = useCallback((e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  }, [onClose]);

  const content = i18n.language?.startsWith("en") ? enContent : helpContent;

  const renderContent = () => {
    switch (activeCategory) {
      case "functions":
        return (
          <div className={styles.section}>
            <h3 className={styles.sectionTitle}>{t("sendBar.helpFunctions")}</h3>
            <p className={styles.sectionIntro}>
              {i18n.language?.startsWith("en")
                ? "The following functions are available in all Lua scripts within the sandboxed VM:"
                : "以下函数可在沙箱化 VM 的所有 Lua 脚本中使用："}
            </p>
            {content.functions.map((fn) => (
              <div key={fn.name} className={styles.entryCard}>
                <div className={styles.entryHeader}>
                  <code className={styles.entryName}>{fn.name}</code>
                  <code className={styles.entrySig}>{fn.signature}</code>
                </div>
                <p className={styles.entryDesc}>{fn.desc}</p>
                {fn.example && (
                  <pre className={styles.entryExample}><code>{fn.example}</code></pre>
                )}
              </div>
            ))}
          </div>
        );

      case "macros":
        return (
          <div className={styles.section}>
            <h3 className={styles.sectionTitle}>{t("sendBar.helpMacros")}</h3>
            <p className={styles.sectionIntro}>
              {i18n.language?.startsWith("en")
                ? "The following macros are available in auto-reply rule reply data fields. Macros are expanded at runtime when a rule triggers. Nested macros (e.g. {{HEXVAL({{RANDOM(0,255)}},2)}}) are supported via iterative expansion (up to 10 rounds)."
                : "以下宏可在自动应答规则的回复数据中使用。宏在规则触发时运行时展开。支持嵌套宏（如 {{HEXVAL({{RANDOM(0,255)}},2)}}），通过迭代展开（最多 10 轮）自动处理。"}
            </p>
            {content.macros.map((m) => (
              <div key={m.name} className={styles.entryCard}>
                <div className={styles.entryHeader}>
                  <code className={styles.entryName}>{m.name}</code>
                  <code className={styles.entrySig}>{m.syntax}</code>
                </div>
                <p className={styles.entryDesc}>{m.desc}</p>
                {m.example && (
                  <pre className={styles.entryExample}><code>{m.example}</code></pre>
                )}
              </div>
            ))}
          </div>
        );

      case "sandbox":
        return (
          <div className={styles.section}>
            <h3 className={styles.sectionTitle}>{content.sandbox.title}</h3>
            <p className={styles.sectionIntro}>{content.sandbox.intro}</p>
            <ul className={styles.restrictionList}>
              {content.sandbox.restrictions.map((r, i) => (
                <li key={i} className={styles.restrictionItem}>{r}</li>
              ))}
            </ul>
            <h4 className={styles.subTitle}>{i18n.language?.startsWith("en") ? "Additional Notes" : "补充说明"}</h4>
            <ul className={styles.noteList}>
              {content.sandbox.notes.map((n, i) => (
                <li key={i} className={styles.noteItem}>{n}</li>
              ))}
            </ul>
          </div>
        );
    }
  };

  if (!isOpen) return null;

  return createPortal(
    <div className={`${styles.overlay} glass-overlay`} onClick={handleOverlayClick}>
      <div className={`${styles.container} liquid-glass`}>
        {/* 标题栏 */}
        <div className={styles.header}>
          <span className={styles.headerTitle}>{t("sendBar.helpTitle")}</span>
          <button className={styles.closeBtn} onClick={onClose}>
            <Icon name="close" size="md" />
          </button>
        </div>

        <div className={styles.body}>
          {/* 左侧导航 */}
          <nav className={styles.nav}>
            {CATEGORIES.map(cat => (
              <button
                key={cat.id}
                className={`${styles.navItem} ${activeCategory === cat.id ? styles.navItemActive : ""}`}
                onClick={() => setActiveCategory(cat.id)}
              >
                <Icon name={cat.icon} size="md" className={styles.navIcon} />
                <span className={styles.navLabel}>{t(cat.labelKey)}</span>
              </button>
            ))}
          </nav>

          {/* 右侧内容 */}
          <div className={styles.content}>
            {renderContent()}
          </div>
        </div>
      </div>
    </div>,
    document.body
  );
}
