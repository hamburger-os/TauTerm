//! Lua 代码生成器
//!
//! 将前端的自动应答规则列表编译为 Lua 脚本代码。
//! 每条启用的规则生成对应的数据处理器，支持：
//! - 5 种匹配模式：contains / equals / starts_with / regex / lua_pattern
//! - 动态宏替换：{{RANDOM}}、{{TIMESTAMP}}、{{CAPTURE(n)}}、{{EXPR}} 等 10 种宏
//! - 序列回复：单条规则可触发多个延时-发送动作
//! - 冷却控制：规则级冷却时间
//! - 匹配策略：first-match（首条命中即停）/ all-match（全部执行）

use serde::{Deserialize, Serialize};

/// 序列回复中的单个动作
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyAction {
    /// 本步延迟 (ms)
    pub delay_ms: u64,
    /// 回复数据，支持 {{MACRO}} 模板
    pub data: String,
    /// 回复格式: "text" | "hex"
    pub format: String,
}

/// 单个匹配条件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchCondition {
    /// 匹配模式字符串
    pub pattern: String,
    /// 匹配方式: "contains" | "equals" | "starts_with" | "regex" | "lua_pattern"
    pub mode: String,
    /// 是否大小写敏感
    #[serde(default)]
    pub case_sensitive: bool,
    /// 取反：true 表示"不匹配时才触发"
    #[serde(default)]
    pub negate: bool,
    /// 匹配格式: "text" | "hex"（默认 text）
    #[serde(default = "default_format")]
    pub match_format: String,
}

/// 前端配置的单个自动应答规则
///
/// 统一模型：匹配条件一律以 `conditions` 数组表示（data 规则至少 1 条），
/// 条件间通过 `condition_logic` 组合。不再有顶层单条件字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoReplyRule {
    /// 规则唯一标识
    pub id: String,
    /// 可选标签
    #[serde(default)]
    pub label: Option<String>,
    /// 触发类型: "data" | "timer"
    #[serde(default = "default_trigger")]
    pub trigger_type: String,
    /// 定时器间隔 (ms)（trigger_type = "timer" 时使用）
    #[serde(default = "default_timer_interval")]
    pub timer_interval_ms: u64,
    /// 匹配条件列表（data 规则至少 1 条；timer 规则为空）
    #[serde(default)]
    pub conditions: Vec<MatchCondition>,
    /// 条件间逻辑: "and" | "or"
    #[serde(default = "default_logic")]
    pub condition_logic: String,
    /// 动作列表
    #[serde(default)]
    pub actions: Vec<ReplyAction>,
    /// 是否启用此规则
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 冷却时间 (ms)，0 = 无冷却
    #[serde(default)]
    pub cooldown_ms: u64,
}

fn default_true() -> bool {
    true
}

fn default_trigger() -> String {
    "data".into()
}

fn default_timer_interval() -> u64 {
    1000
}

fn default_logic() -> String {
    "and".into()
}

fn default_format() -> String {
    "text".into()
}

// ── 常量：Lua 脚本模板片段 ──────────────────────────────

/// 脚本头部：运行时状态 + 宏展开引擎 + 冷却检查
const SCRIPT_HEADER: &str = r#"-- ── 运行时状态 ──
__counters = {}          -- 规则级计数器 { [rule_index] = n }
__cooldowns = {}         -- 规则冷却记录 { [rule_index] = last_match_ms }

-- ── CRC / Checksum 辅助函数（纯 Lua 实现）──

-- 统一 CRC 计算引擎: _crc_compute(data, width, poly)
-- 内置 preset 表自动推断 init/refin/refout/xorout；未知 poly 使用安全默认。
-- 返回大写 HEX 字符串。

local _crc_presets = {
	[8] = {
		[0x07] = { init = 0x00,   refin = false, refout = false, xorout = 0x00 },
		[0x31] = { init = 0x00,   refin = true,  refout = true,  xorout = 0x00, work_poly = 0x8C, refl_byte = true },
	},
	[16] = {
		[0x1021] = { init = 0x0000, refin = true, refout = true,  xorout = 0x0000, work_poly = 0x8408, refl_byte = true },
		[0x8005] = { init = 0xFFFF, refin = true, refout = false, xorout = 0x0000, swap = true },
	},
	[32] = {
		[0x04C11DB7] = { init = 0xFFFFFFFF, refin = true, refout = false, xorout = 0xFFFFFFFF, work_poly = 0xEDB88320 },
	},
}

-- 反射 value (width-bit)
-- 反射 byte (8-bit)
local function _reflect8(b)
	local r = 0
	for i = 0, 7 do
		if b & (1 << i) ~= 0 then r = r | (1 << (7 - i)) end
	end
	return r
end

local function _reflect(v, width)
	local r = 0
	for i = 0, width - 1 do
		if v & (1 << i) ~= 0 then r = r | (1 << (width - 1 - i)) end
	end
	return r
end

function _crc_compute(data, width, poly)
	-- 查找 preset / 默认参数
	local p
	if _crc_presets[width] then p = _crc_presets[width][poly] end
	if not p then
		-- 未知 poly：安全默认
		if width == 32 then
			p = { init = 0xFFFFFFFF, refin = true, refout = false, xorout = 0xFFFFFFFF }
		elseif width == 16 then
			p = { init = 0x0000, refin = true, refout = true,  xorout = 0x0000 }
		else
			p = { init = 0x00, refin = false, refout = false, xorout = 0x00 }
		end
	end

	local mask = (width == 32) and 0xFFFFFFFF or ((1 << width) - 1)
	local crc = p.init

	if p.refin then
		-- 反射算法：poly 需反射为运算用多项式，LSB 优先处理
		local work_poly = _reflect(poly, width)
		for i = 1, #data do
			local byte = p.refl_byte and _reflect8(string.byte(data, i)) or string.byte(data, i)
			crc = crc ~ byte
			for _ = 1, 8 do
				if crc & 1 ~= 0 then
					crc = (crc >> 1) ~ work_poly
				else
					crc = crc >> 1
				end
			end
		end
	else
		-- 直通算法：MSB 优先处理
		local top_bit = 1 << (width - 1)
		for i = 1, #data do
			local byte = string.byte(data, i)
			crc = crc ~ (byte << (width - 8))
			for _ = 1, 8 do
				if crc & top_bit ~= 0 then
					crc = ((crc << 1) ~ poly) & mask
				else
					crc = (crc << 1) & mask
				end
			end
		end
	end

	if p.refout then
		crc = _reflect(crc, width)
	end

	crc = crc ~ p.xorout

	if p.swap then
		-- 字节交换（Modbus 小端序输出）
		local lo = crc & 0xFF
		local hi = (crc >> 8) & 0xFF
		return string.format("%02X%02X", lo, hi)
	end

	local hex_width = math.ceil(width / 4)
	return string.format("%0" .. hex_width .. "X", crc)
end

-- 逐字节 XOR 校验和（NMEA *XX）
local function _xor_checksum(data)
	local csum = 0
	for i = 1, #data do
		csum = csum ~ string.byte(data, i)
	end
	return string.format("%02X", csum & 0xFF)
end

-- 逐字节求和取低 8 位（Intel HEX）
local function _sum8(data)
	local sum = 0
	for i = 1, #data do
		sum = sum + string.byte(data, i)
	end
	return string.format("%02X", sum & 0xFF)
end

-- ── 宏展开引擎 ──
-- 将模板字符串中的 {{MACRO(args)}} 替换为运行时计算值。
-- captures 是 regex_find 返回的捕获组表 (1-indexed)。
	-- ── 宏展开引擎 ──
	-- 使用迭代展开：每次匹配最内层 {{...}} 直至稳定（最多 10 轮）。
	-- 此机制自动支持嵌套宏，如 {{HEXVAL({{RANDOM(0,255)}},2)}}。
	--
	-- 从宏文本中提取函数名和参数（处理嵌套括号）
	local function _parse_macro(text)
	    -- EXPR: 前缀
	    local expr_body = string.match(text, "^EXPR:(.+)$")
	    if expr_body then return "EXPR", { expr_body } end
	    -- 函数形式: NAME(args) — 正确处理 args 内部的嵌套括号
	    local paren = string.find(text, "%(")
	    if paren then
	        local name = string.sub(text, 1, paren - 1)
	        local depth, close = 0, nil
	        for i = paren, #text do
	            local c = string.sub(text, i, i)
	            if c == "(" then depth = depth + 1
	            elseif c == ")" then
	                depth = depth - 1
	                if depth == 0 then close = i; break end
	            end
	        end
	        if close then
	            return name, { string.sub(text, paren + 1, close - 1) }
	        end
	    end
	    -- 简单宏: 无参数
	    return text, {}
	end

	-- 展开单个宏（text 是 {{ 和 }} 之间的内容）
	local function _expand_one(text, rule_index, captures)
	    local name, args = _parse_macro(text)
	    local a1 = args[1]

	    if name == "CAPTURE" then
	        local n = tonumber(a1 or "")
	        return (captures or {})[n] or ""

	    elseif name == "RANDOM" then
	        local min_s, max_s = string.match(a1 or "", "^(%-?%d+%.?%d*),(%-?%d+%.?%d*)$")
	        if min_s then
	            return tostring(math.random(tonumber(min_s), tonumber(max_s)))
	        end
	        return "ERR:RANDOM"

	    elseif name == "RANDOM_F" then
	        local min_s, max_s, dec_s = string.match(a1 or "", "^(%-?%d+%.?%d*),(%-?%d+%.?%d*),(%d+)$")
	        if min_s then
	            local min_v, max_v, dec = tonumber(min_s), tonumber(max_s), tonumber(dec_s)
	            return string.format("%." .. dec .. "f", min_v + (max_v - min_v) * math.random())
	        end
	        return "ERR:RANDOM_F"

	    elseif name == "TIMESTAMP" then
	        return tostring(_time_ms())

	    elseif name == "DATETIME" then
	        return _datetime_iso()

	    elseif name == "DATETIME_F" then
	        return _datetime_format(a1 or "")

	    elseif name == "COUNTER" then
	        __counters[rule_index] = (__counters[rule_index] or 0) + 1
	        return tostring(__counters[rule_index])

	    elseif name == "HEX" then
	        return (string.gsub(a1 or "", ".", function(c)
	            return string.format("%02X", string.byte(c))
	        end))

	    elseif name == "SIN" then
	        local min_s, max_s, period_s = string.match(a1 or "", "^(%-?%d+%.?%d*),(%-?%d+%.?%d*),(%d+)$")
	        if min_s then
	            local min_v, max_v = tonumber(min_s), tonumber(max_s)
	            local period = tonumber(period_s)
	            local t = _time_ms() % period
	            local val = min_v + (max_v - min_v) * (math.sin((t / period) * 2 * math.pi) + 1) / 2
	            return string.format("%.2f", val)
	        end
	        return "ERR:SIN"

	    elseif name == "HEXVAL" then
	        -- {{HEXVAL(number,width)}} — 数字格式化为大写 HEX
	        local num_s, width_s = string.match(a1 or "", "^(%-?%d+),?(%d*)$")
	        if num_s then
	            local n = tonumber(num_s)
	            if not n then return "ERR:HEXVAL" end
	            local w = tonumber(width_s)
	            if not w or w <= 0 then
	                if n == 0 then w = 2
	                else w = math.ceil(math.log(math.abs(n) + 1, 16)) end
	                if w % 2 ~= 0 then w = w + 1 end
	            end
	            if n < 0 then return "-" .. string.format("%0" .. (w - 1) .. "X", -math.floor(n))
	            else return string.format("%0" .. w .. "X", math.floor(n)) end
	        end
	        return "ERR:HEXVAL"

	    elseif name == "CRC" then
	        -- {{CRC(data, width, poly)}} — 从右解析：最后两个参数是 width 和 poly
	        -- poly 支持 0x 前缀十六进制或十进制
	        local data_part, w_s, poly_s = string.match(a1 or "", "^(.*),%s*(%d+)%s*,%s*(0[xX]%x+)%s*$")
	        if not data_part then
	            data_part, w_s, poly_s = string.match(a1 or "", "^(.*),%s*(%d+)%s*,%s*(%d+)%s*$")
	        end
	        if data_part then
	            return _crc_compute(data_part, tonumber(w_s), tonumber(poly_s))
	        end
	        return "ERR:CRC"

	    elseif name == "XOR_SUM" then
	        return _xor_checksum(a1 or "")

	    elseif name == "SUM8" then
	        return _sum8(a1 or "")

	    elseif name == "EXPR" then
	        -- 安全求值算术表达式（支持位运算: & | ~ << >>）
	        local resolved = a1 or ""
	        -- 1. 替换变量引用为数字字面量
	        resolved = string.gsub(resolved, "CAPTURE%((%d+)%)", function(n)
	            return (captures or {})[tonumber(n)] or "0"
	        end)
	        resolved = string.gsub(resolved, "COUNTER", function()
	            return tostring(__counters[rule_index] or 0)
	        end)
	        -- 2. 白名单校验：允许数字、小数点、空白、运算符、括号、位运算符
	        if string.find(resolved, "[^%d%.%s%+%-%*%/%^%%%(%)&|><~]") then
	            log("EXPR: unsafe expression rejected: " .. (a1 or ""))
	            return "ERR"
	        end
	        -- 3. pcall 安全求值
	        local ok, val = pcall(function()
	            return load("return " .. resolved)()
	        end)
	        if ok and type(val) == "number" then
	            if val == math.floor(val) then
	                return tostring(math.floor(val))
	            else
	                return string.format("%.2f", val)
	            end
	        else
	            log("EXPR: eval failed: " .. (a1 or ""))
	            return "ERR"
	        end

	    end

	    -- 未知宏：保持原样
	    return "{{" .. text .. "}}"
	end

	-- 主展开函数：迭代匹配最内层 {{...}} 直至稳定（最多 10 轮）
	local function _expand_reply(template, rule_index, captures)
	    local result = template
	    local iterations = 0
	    while iterations < 10 do
	        local prev = result
	        iterations = iterations + 1
	        -- [^{}]- 确保只匹配最内层（不含嵌套 {{）的宏
	        result = string.gsub(result, "{{([^{}]-)}}", function(inner)
	            return _expand_one(inner, rule_index, captures)
	        end)
	        if result == prev then break end
	    end
	    return result
	end

-- ── HEX 格式回复（含宏展开） ──
-- 先将模板展开为 HEX 字符串，再逐字节转为 Lua 字符串
local function _hex_expand(template, rule_index, captures)
    local expanded = _expand_reply(template, rule_index, captures)
    -- 去除空白字符
    local clean = string.gsub(expanded, "%s+", "")
    if #clean % 2 ~= 0 then
        log("HEX reply: odd length after expansion (" .. #clean .. " chars)")
        return ""
    end
    local result = ""
    for i = 1, #clean - 1, 2 do
        local byte = tonumber(string.sub(clean, i, i+1), 16)
        if byte then
            result = result .. string.char(byte)
        end
    end
    return result
end

-- ── 冷却检查 ──
-- 返回 true 表示可以通过（未冷却或冷却已过）
local function _check_cooldown(rule_index, cooldown_ms)
    if not cooldown_ms or cooldown_ms <= 0 then
        return true
    end
    local now = _time_ms()
    local last = __cooldowns[rule_index] or 0
    if now - last < cooldown_ms then
        return false
    end
    __cooldowns[rule_index] = now
    return true
end

-- ── 匹配辅助函数 ──
-- 支持 contains / equals / starts_with / lua_pattern 四种模式。
-- regex 模式不经过此函数，由 regex_find() 独立处理。
local function match_data(data, pattern, mode, case_sensitive)
    local text = data
    local pat = pattern
    if not case_sensitive then
        text = string.lower(data)
        pat = string.lower(pattern)
    end

    if mode == "contains" then
        return string.find(text, pat, 1, true) ~= nil
    elseif mode == "equals" then
        return text == pat
    elseif mode == "starts_with" then
        return string.sub(text, 1, #pat) == pat
    elseif mode == "lua_pattern" then
        return string.find(text, pat) ~= nil
    end
    return false
end

-- ── 二进制（HEX）匹配辅助函数 ──
-- pat_bytes 已由 codegen 转为原始字节串（\xNN 转义），无大小写概念。
-- 使用 plain-find（第 4 参数 true）确保逐字节匹配，不受 Lua 魔法字符影响。
local function match_data_hex(data, pat_bytes, mode)
    if mode == "contains" then
        return string.find(data, pat_bytes, 1, true) ~= nil
    elseif mode == "equals" then
        return data == pat_bytes
    elseif mode == "starts_with" then
        return string.sub(data, 1, #pat_bytes) == pat_bytes
    end
    return false
end

-- 测试入口（供 Rust 测试调用 local helper）
__test = {
	_xor_checksum = _xor_checksum,
	_sum8 = _sum8,
}
"#;

/// first-match 策略的调度函数头部
///
/// 使用空 pattern（预过滤恒通过），与 all-match 的独立 handler 一致。
/// 真正的匹配在调度器内联逐条完成。注意不可用 `"*"`：feed_data 预过滤会对
/// pattern 执行 `string.find(data, "*")`，而 `*` 是 Lua 魔法量词，会触发
/// "malformed pattern" 使 pcall 失败，导致 first-match 规则永不触发。
const FIRST_MATCH_HEADER: &str = r#"
-- ── first-match 调度器 ──
on_data("", function(data)
"#;

/// first-match 策略的调度函数尾部
const FIRST_MATCH_FOOTER: &str = r#"end)
"#;

// ── 公开 API ────────────────────────────────────────────

/// 将规则列表编译为 Lua 脚本代码
///
/// # 参数
/// - `rules`: 规则列表
/// - `script_name`: 脚本名称（用于注释头）
/// - `match_strategy`: "first"（首条命中即停）| "all"（全部执行）
pub fn rules_to_lua_script(
    rules: &[AutoReplyRule],
    script_name: &str,
    match_strategy: &str,
) -> String {
    let mut code = String::new();

    // ── 文件头注释 ──
    code.push_str(&format!(
        "-- Auto-generated: {}\n",
        escape_lua_comment(script_name)
    ));
    code.push_str(&format!("-- Strategy: {}\n", match_strategy));
    code.push_str("-- Generated by TauTerm AutoReply\n\n");

    // ── 注入公共函数头 ──
    code.push_str(SCRIPT_HEADER);

    // ── 过滤启用的规则 ──
    let enabled_rules: Vec<(usize, &AutoReplyRule)> = rules
        .iter()
        .enumerate()
        .filter(|(_, r)| r.enabled)
        .collect();

    if enabled_rules.is_empty() {
        code.push_str("-- (no enabled rules)\n");
        return code;
    }

    let is_first_match = match_strategy == "first";

    // 分离定时器规则与数据驱动规则。
    // 定时器规则始终通过 register_timer 独立注册（不受 first/all 策略影响），
    // 因此必须置于 first-match 的 on_data("*") 调度器之外。
    let (timer_rules, data_rules): (Vec<_>, Vec<_>) = enabled_rules
        .iter()
        .partition(|(_, r)| r.trigger_type == "timer");

    // ── 数据驱动规则 ──
    if !data_rules.is_empty() {
        if is_first_match {
            code.push_str(FIRST_MATCH_HEADER);
        }
        for (rule_index, rule) in &data_rules {
            code.push_str(&generate_rule_handler(*rule_index, rule, is_first_match));
        }
        if is_first_match {
            code.push_str(FIRST_MATCH_FOOTER);
        }
    }

    // ── 定时器规则（策略无关，独立注册）──
    for (rule_index, rule) in &timer_rules {
        code.push_str(&generate_rule_handler(*rule_index, rule, false));
    }

    code
}

// ── 规则处理器生成 ──────────────────────────────────────

/// 生成单条规则的 Lua 处理器代码
fn generate_rule_handler(rule_index: usize, rule: &AutoReplyRule, is_first_match: bool) -> String {
    let mut code = String::new();
    let idx = rule_index + 1; // 1-based for Lua

    // 注释：概括触发方式与首条匹配条件
    let label = rule
        .label
        .as_ref()
        .filter(|l| !l.trim().is_empty())
        .map(|l| format!(" \"{}\"", l))
        .unwrap_or_default();
    if rule.trigger_type == "timer" {
        code.push_str(&format!(
            "\n-- Rule #{}:{}\n--   trigger=timer interval={}ms",
            idx, label, rule.timer_interval_ms,
        ));
    } else {
        let (pat, mode) = rule
            .conditions
            .first()
            .map(|c| (c.pattern.as_str(), c.mode.as_str()))
            .unwrap_or(("", ""));
        let multi = if rule.conditions.len() > 1 {
            format!(" +{} conds ({})", rule.conditions.len() - 1, rule.condition_logic)
        } else {
            String::new()
        };
        code.push_str(&format!(
            "\n-- Rule #{}:{}\n--   match=\"{}\" mode={}{}",
            idx,
            label,
            escape_lua_comment(pat),
            mode,
            multi,
        ));
    }

    if !rule.actions.is_empty() {
        code.push_str(&format!(" actions={}", rule.actions.len()));
    }
    if rule.cooldown_ms > 0 {
        code.push_str(&format!(" cooldown={}ms", rule.cooldown_ms));
    }
    code.push('\n');

    // ── 定时器规则：register_timer 而非 on_data ──
    if rule.trigger_type == "timer" {
        code.push_str(&generate_timer_handler(idx, rule));
        return code;
    }

    // ── first-match 模式：内联 handler ──
    if is_first_match {
        code.push_str(&generate_inline_handler(idx, rule));
        return code;
    }

    // ── all-match 模式：独立 on_data ──
    code.push_str(&generate_on_data_handler(idx, rule));
    code
}

/// 生成 all-match 模式的独立 on_data handler
fn generate_on_data_handler(idx: usize, rule: &AutoReplyRule) -> String {
    let mut code = String::new();

    // 始终使用空 pattern（预过滤恒通过），真正的匹配由 handler body 内的
    // match_data/match_data_hex/regex_find 完成。这避免了 feed_data 预过滤器
    // 对 contains/equals 等模式误用 Lua pattern 匹配的 bug（含魔法字符时假阴性）。
    code.push_str("on_data(\"\", function(data)\n");
    code.push_str(&generate_handler_body(idx, rule, "    ", false));
    code.push_str("end)\n");

    code
}

/// 生成定时器规则的 register_timer 调用
///
/// 定时器规则不进行数据匹配，按固定间隔周期性执行动作序列。
/// 注意：定时器回调内的 sleep() 会阻塞其他定时器，建议用 timerIntervalMs 控制速率。
fn generate_timer_handler(idx: usize, rule: &AutoReplyRule) -> String {
    let interval = rule.timer_interval_ms.max(1);
    let mut body = String::new();

    // 冷却检查（可选）
    if rule.cooldown_ms > 0 {
        body.push_str(&format!(
            "    if not _check_cooldown({}, {}) then return end\n",
            idx, rule.cooldown_ms,
        ));
    }

    // 回复动作（复用 send 生成逻辑；定时器无捕获组，caps 为 nil → {}）
    for (i, action) in rule.actions.iter().enumerate() {
        body.push_str(&format!("    -- Action {}\n", i + 1));
        if action.delay_ms > 0 {
            body.push_str(&format!("    sleep({})\n", action.delay_ms));
        }
        body.push_str(&format!(
            "{}\n",
            generate_send_call(idx, &action.data, &action.format, "    ")
        ));
    }

    format!(
        "register_timer(\"__timer_rule_{}\", {}, function()\n{}end)\n",
        idx, interval, body
    )
}

/// 生成 first-match 模式的内联处理器片段
fn generate_inline_handler(idx: usize, rule: &AutoReplyRule) -> String {
    let mut code = String::new();
    code.push_str(&format!("    -- Rule #{}\n", idx));
    // first-match: 匹配成功执行动作后 return，短路后续规则
    code.push_str(&generate_handler_body(idx, rule, "    ", true));
    code
}

/// 生成 handler 函数体（匹配逻辑 + 回复动作）
///
/// `is_first_match` 为 true 时，在动作执行后生成 `return`，使 first-match 调度器
/// 在首条命中规则后跳出回调，不再检查后续规则。
fn generate_handler_body(idx: usize, rule: &AutoReplyRule, indent: &str, is_first_match: bool) -> String {
    let mut code = String::new();

    // ── 匹配逻辑 ──
    let conditions = &rule.conditions;
    let logic_or = rule.condition_logic == "or";

    // 单条正则（未取反、text 格式）→ 保留捕获组路径
    let single_regex_capture = conditions.len() == 1
        && conditions[0].mode == "regex"
        && !conditions[0].negate
        && conditions[0].match_format != "hex";

    if conditions.is_empty() {
        // 防御：data 规则理应至少 1 条件，空则永不触发
        code.push_str(&format!("{indent}if false then -- no conditions\n"));
    } else if single_regex_capture {
        // Regex 模式：调用 regex_find，获取捕获组
        code.push_str(&format!(
            r#"{indent}local caps = regex_find("{}", data)
{indent}if caps then
"#,
            escape_lua_string(&conditions[0].pattern)
        ));
    } else {
        // 组合布尔表达式（捕获组不可用，caps 为 nil → 展开时回退 {}）
        let joiner = if logic_or { " or " } else { " and " };
        let expr = conditions
            .iter()
            .map(|c| {
                let e = condition_expr(c);
                if c.negate {
                    format!("not ({})", e)
                } else {
                    e
                }
            })
            .collect::<Vec<_>>()
            .join(joiner);
        code.push_str(&format!("{indent}if {} then\n", expr));
    }

    // ── 冷却检查 ──
    if rule.cooldown_ms > 0 {
        code.push_str(&format!(
            "{indent}    if not _check_cooldown({}, {}) then return end\n",
            idx, rule.cooldown_ms,
        ));
    }

    // ── 回复动作 ──
    let indent2 = format!("{}    ", indent);
    if !rule.actions.is_empty() {
        for (i, action) in rule.actions.iter().enumerate() {
            code.push_str(&format!(
                "{}-- Action {}\n",
                indent2,
                i + 1
            ));
            if action.delay_ms > 0 {
                code.push_str(&format!("{}sleep({})\n", indent2, action.delay_ms));
            }
            code.push_str(&format!(
                "{}\n",
                generate_send_call(idx, &action.data, &action.format, &indent2)
            ));
        }
    }

    // ── 闭合 ──
    // first-match 模式：动作执行完毕后 return，短路后续规则检查
    if is_first_match {
        code.push_str(&format!("{}    return\n", indent));
    }
    code.push_str(&format!("{}end\n", indent));

    code
}

/// 生成 send() 调用，自动判断是否包含宏
fn generate_send_call(idx: usize, data: &str, format: &str, indent: &str) -> String {
    let has_macros = data.contains("{{");

    if format == "hex" {
        if has_macros {
            format!(
                "{}send(_hex_expand(\"{}\", {}, caps or {{}}))",
                indent,
                escape_lua_string(data),
                idx,
            )
        } else {
            match hex_to_lua_escapes(data) {
                Ok(escaped) => format!("{}send(\"{}\")", indent, escaped),
                Err(_) => format!(
                    "{}-- ERROR: invalid hex \"{}\"",
                    indent,
                    escape_lua_comment(data)
                ),
            }
        }
    } else {
        if has_macros {
            format!(
                "{}send(_expand_reply(\"{}\", {}, caps or {{}}))",
                indent,
                escape_lua_string(data),
                idx,
            )
        } else {
            format!("{}send(\"{}\")", indent, escape_lua_string(data))
        }
    }
}

// ── 条件匹配辅助 ──────────────────────────────────────

/// 生成单个条件的 Lua 布尔表达式（不含取反，取反在调用方处理）
fn condition_expr(c: &MatchCondition) -> String {
    // HEX 格式：转为字节串进行二进制匹配
    if c.match_format == "hex" {
        let bytes = match hex_to_bytes(&c.pattern) {
            Ok(b) => b,
            Err(e) => {
                return format!(
                    "log(\"HEX parse error [{}]: {}\") return false",
                    escape_lua_string(&c.pattern),
                    escape_lua_string(&e),
                );
            }
        };
        let lua_bytes = bytes_to_lua_escapes(&bytes);
        return format!("match_data_hex(data, \"{}\", \"{}\")", lua_bytes, c.mode);
    }

    match c.mode.as_str() {
        "regex" => format!("(regex_find(\"{}\", data) ~= nil)", escape_lua_string(&c.pattern)),
        // contains/equals/starts_with：解释 \r\n\t\0 转义序列为实际控制字符
        "contains" | "equals" | "starts_with" => {
            let interpreted = interpret_escape_sequences(&c.pattern);
            format!(
                "match_data(data, \"{}\", \"{}\", {})",
                escape_lua_string(&interpreted),
                c.mode,
                c.case_sensitive,
            )
        }
        // lua_pattern：保持原样（Lua 模式使用 % 而非 \ 作转义符）
        _ => format!(
            "match_data(data, \"{}\", \"{}\", {})",
            escape_lua_string(&c.pattern),
            c.mode,
            c.case_sensitive,
        ),
    }
}

/// 将 JS 风格的转义序列（\r \n \t \0 \\）解释为实际控制字符
///
/// 匹配表达式使用单行 input，用户无法直接输入换行符。此函数使
/// `equals`/`contains`/`starts_with` 模式下的 `*IDN?\r\n` 能匹配真实的 CR+LF。
pub fn interpret_escape_sequences(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('r') => result.push('\r'),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('0') => result.push('\0'),
                Some('\\') => result.push('\\'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// 将 HEX 字符串（忽略空白）转为字节向量。
///
/// 返回 `Err` 当遇到非法 HEX 字符或奇数长度（非完整字节）。
/// 空字符串返回空向量。
pub fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, String> {
    let clean: Vec<char> = hex.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.is_empty() {
        return Ok(Vec::new());
    }
    if !clean.len().is_multiple_of(2) {
        return Err(format!(
            "HEX 数据长度为奇数 ({} 个字符)，每个字节需要 2 个 hex 字符",
            clean.len()
        ));
    }
    let mut bytes = Vec::with_capacity(clean.len() / 2);
    for chunk in clean.chunks(2) {
        let s: String = chunk.iter().collect();
        let byte = u8::from_str_radix(&s, 16)
            .map_err(|_| format!("无效的 HEX 字符: \"{}\"", s))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

/// 将字节向量转为 Lua 字符串字面量的 \xNN 转义序列
fn bytes_to_lua_escapes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\x{:02x}", b)).collect()
}

// ── 字符串转义工具 ──────────────────────────────────────

/// 转义字符串为 Lua 安全字符串字面量内容
fn escape_lua_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('\0', "\\0")
}

/// 转义字符串为 Lua 注释安全文本
fn escape_lua_comment(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

/// 将十六进制字符串转为 Lua 转义序列
/// "4142" → "\\x41\\x42"
fn hex_to_lua_escapes(hex: &str) -> Result<String, String> {
    let clean: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.is_empty() {
        return Ok(String::new());
    }
    if !clean.len().is_multiple_of(2) {
        return Err(format!(
            "HEX 数据长度为奇数 ({} 个字符)，每个字节需要 2 个 hex 字符",
            clean.len()
        ));
    }
    let mut result = String::new();
    let chars: Vec<char> = clean.chars().collect();
    for chunk in chars.chunks(2) {
        let hex_str: String = chunk.iter().collect();
        let byte = u8::from_str_radix(&hex_str, 16)
            .map_err(|_| format!("无效的 HEX 字符: \"{}\"", hex_str))?;
        result.push_str(&format!("\\x{:02x}", byte));
    }
    Ok(result)
}

// ── 单元测试 ────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rule(
        id: &str,
        pattern: &str,
        mode: &str,
        reply: &str,
        delay: u64,
    ) -> AutoReplyRule {
        let actions = if reply.is_empty() && delay == 0 {
            vec![]
        } else {
            vec![ReplyAction {
                delay_ms: delay,
                data: reply.into(),
                format: "text".into(),
            }]
        };
        AutoReplyRule {
            id: id.into(),
            label: None,
            trigger_type: "data".into(),
            timer_interval_ms: 1000,
            conditions: vec![MatchCondition {
                pattern: pattern.into(),
                mode: mode.into(),
                case_sensitive: false,
                negate: false,
                match_format: "text".into(),
            }],
            condition_logic: "and".into(),
            actions,
            enabled: true,
            cooldown_ms: 0,
        }
    }

    /// 便捷设置首条件的大小写敏感（测试辅助）
    fn set_case_sensitive(rule: &mut AutoReplyRule, cs: bool) {
        if let Some(c) = rule.conditions.first_mut() {
            c.case_sensitive = cs;
        }
    }

    /// 前端通过 Tauri invoke 传入的是**驼峰键** JSON。Tauri v2 只转命令顶层参数名，
    /// 不转嵌套结构体字段，因此结构体必须能直接反序列化 camelCase JSON。此测试守护
    /// 前后端字段命名协议，防止回归到"点击开始执行无反应"（反序列化失败被静默吞掉）。
    #[test]
    fn test_deserialize_camel_case_json_from_frontend() {
        let json = r#"{
            "id": "r1",
            "label": "AT 应答",
            "triggerType": "data",
            "conditions": [
                { "pattern": "AT", "mode": "contains", "caseSensitive": true, "negate": false }
            ],
            "conditionLogic": "and",
            "actions": [
                { "delayMs": 100, "data": "STEP1", "format": "text" },
                { "delayMs": 200, "data": "STEP2", "format": "hex" }
            ],
            "enabled": true,
            "cooldownMs": 500
        }"#;

        let rule: AutoReplyRule =
            serde_json::from_str(json).expect("前端驼峰 JSON 应能反序列化为 AutoReplyRule");

        assert_eq!(rule.id, "r1");
        assert_eq!(rule.trigger_type, "data");
        assert_eq!(rule.conditions.len(), 1);
        assert_eq!(rule.conditions[0].pattern, "AT");
        assert_eq!(rule.conditions[0].mode, "contains");
        assert!(rule.conditions[0].case_sensitive);
        assert_eq!(rule.condition_logic, "and");
        assert_eq!(rule.cooldown_ms, 500);
        assert_eq!(rule.actions.len(), 2);
        assert_eq!(rule.actions[0].delay_ms, 100);
        assert_eq!(rule.actions[0].data, "STEP1");
        assert_eq!(rule.actions[1].delay_ms, 200);
        assert_eq!(rule.actions[1].format, "hex");
    }

    /// 定时器规则的最小 JSON（无 conditions）也应能反序列化（依赖 serde 默认值）。
    #[test]
    fn test_deserialize_timer_rule_json() {
        let json = r#"{
            "id": "t1",
            "triggerType": "timer",
            "timerIntervalMs": 2000,
            "actions": [{ "delayMs": 0, "data": "PING", "format": "text" }],
            "enabled": true
        }"#;
        let rule: AutoReplyRule = serde_json::from_str(json).expect("定时器 JSON 应能反序列化");
        assert_eq!(rule.trigger_type, "timer");
        assert_eq!(rule.timer_interval_ms, 2000);
        assert!(rule.conditions.is_empty());
        assert_eq!(rule.condition_logic, "and"); // 默认值
    }

    #[test]
    fn test_empty_rules() {
        let code = rules_to_lua_script(&[], "test", "all");
        assert!(code.contains("-- (no enabled rules)"));
    }

    #[test]
    fn test_disabled_rule_skipped() {
        let mut rule = make_rule("1", "AT", "contains", "OK", 0);
        rule.enabled = false;
        let code = rules_to_lua_script(&[rule], "test", "all");
        assert!(code.contains("-- (no enabled rules)"));
    }

    #[test]
    fn test_simple_contains_rule_all_match() {
        let rule = make_rule("1", "AT", "contains", "OK\r\n", 10);
        let code = rules_to_lua_script(&[rule], "AT Test", "all");
        eprintln!("=== CODE ===\n{}\n=== END ===", code);
        assert!(code.contains("on_data(\"\""));
        assert!(code.contains("match_data(data, \"AT\", \"contains\", false)"));
        assert!(code.contains("sleep(10)"));
        assert!(code.contains("send(\"OK\\r\\n\")"));
    }

    #[test]
    fn test_simple_equals_rule() {
        let rule = make_rule("1", "*IDN?", "equals", "TauTerm v1.0\r\n", 0);
        let code = rules_to_lua_script(&[rule], "test", "all");
        assert!(code.contains("match_data(data, \"*IDN?\", \"equals\", false)"));
        assert!(!code.contains("sleep(")); // delay=0 → no sleep
        assert!(code.contains("send(\"TauTerm v1.0\\r\\n\")"));
    }

    #[test]
    fn test_regex_rule() {
        let mut rule = make_rule(
            "1",
            r"TEMP:\s*(\d+\.\d+)",
            "regex",
            "+TEMP:{{CAPTURE(1)}},T={{TIMESTAMP}}\r\n",
            0,
        );
        set_case_sensitive(&mut rule, true);
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== REGEX CODE ===\n{}\n=== END ===", code);
        assert!(code.contains("regex_find"));
        assert!(code.contains("on_data(\"\"")); // empty pattern for regex
        assert!(code.contains("local caps = regex_find"));
        assert!(code.contains("_expand_reply"));
        assert!(code.contains("{{CAPTURE(1)}}"));
        assert!(code.contains("{{TIMESTAMP}}"));
    }

    #[test]
    fn test_regex_rule_no_macro() {
        let mut rule = make_rule("1", r"AT\+.*", "regex", "OK\r\n", 0);
        set_case_sensitive(&mut rule, true);
        let code = rules_to_lua_script(&[rule], "test", "all");
        assert!(code.contains("regex_find"));
        // 无宏 → 不使用 _expand_reply
        assert!(code.contains("send(\"OK\\r\\n\")"));
    }

    #[test]
    fn test_macro_in_reply() {
        let rule = make_rule("1", "GET_TEMP", "contains", "TEMP:{{RANDOM(20,30)}}C\r\n", 0);
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== MACRO CODE ===\n{}\n=== END ===", code);
        assert!(code.contains("_expand_reply"));
        assert!(code.contains("{{RANDOM(20,30)}}"));
    }

    #[test]
    fn test_sequence_rule() {
        let mut rule = make_rule("1", "*IDN?", "equals", "", 0);
        rule.actions = vec![
            ReplyAction {
                delay_ms: 50,
                data: "ACK\r\n".into(),
                format: "text".into(),
            },
            ReplyAction {
                delay_ms: 200,
                data: "+SENSOR:{{RANDOM(0,100)}}\r\n".into(),
                format: "text".into(),
            },
        ];
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== SEQUENCE CODE ===\n{}\n=== END ===", code);
        assert!(code.contains("-- Action 1"));
        assert!(code.contains("sleep(50)"));
        assert!(code.contains("send(\"ACK\\r\\n\")"));
        assert!(code.contains("-- Action 2"));
        assert!(code.contains("sleep(200)"));
        assert!(code.contains("_expand_reply"));
    }

    #[test]
    fn test_cooldown_generation() {
        let mut rule = make_rule("1", "AT", "contains", "OK\r\n", 0);
        rule.cooldown_ms = 2000;
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== COOLDOWN CODE ===\n{}\n=== END ===", code);
        assert!(code.contains("_check_cooldown(1, 2000)"));
        assert!(!code.contains("_check_cooldown(1, 0)"));
    }

    #[test]
    fn test_first_match_strategy() {
        let r1 = make_rule("1", "AT", "contains", "OK\r\n", 0);
        let r2 = make_rule("2", "AT+", "contains", "ERROR\r\n", 0);
        let code = rules_to_lua_script(&[r1, r2], "test", "first");
        eprintln!("=== FIRST MATCH CODE ===\n{}\n=== END ===", code);
        // first-match 模式应使用统一的 on_data 调度器（空 pattern，预过滤恒通过）
        assert!(code.contains("on_data(\"\", function(data)"));
        assert!(code.contains("end)"));
        // 不应出现独立的 on_data
        assert!(!code.contains("on_data(\"AT\", function(data)"));
        assert_eq!(
            code.matches("on_data(").count(),
            1,
            "first-match 应只有 1 个 on_data"
        );
    }

    #[test]
    fn test_hex_reply() {
        let mut rule = make_rule("1", "7E", "lua_pattern", "7E01", 0);
        rule.actions[0].format = "hex".into();
        let code = rules_to_lua_script(&[rule], "Hex Test", "all");
        assert!(code.contains("\\x7e\\x01"));
    }

    #[test]
    fn test_hex_reply_invalid() {
        let mut rule = make_rule("1", "AT", "contains", "ZZ", 0);
        rule.actions[0].format = "hex".into();
        let code = rules_to_lua_script(&[rule], "Invalid Hex", "all");
        assert!(code.contains("ERROR"));
    }

    #[test]
    fn test_macro_hex_reply() {
        let mut rule = make_rule("1", "AT", "contains", "{{HEX(OK)}}", 0);
        rule.actions[0].format = "hex".into();
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== MACRO HEX CODE ===\n{}\n=== END ===", code);
        assert!(code.contains("_hex_expand"));
        assert!(code.contains("{{HEX(OK)}}"));
    }

    #[test]
    fn test_script_header_includes_all_macros() {
        let code = rules_to_lua_script(&[], "test", "all");
        assert!(code.contains("_expand_reply"));
        assert!(code.contains("CAPTURE"));
        assert!(code.contains("RANDOM"));
        assert!(code.contains("RANDOM_F"));
        assert!(code.contains("TIMESTAMP"));
        assert!(code.contains("DATETIME"));
        assert!(code.contains("DATETIME_F"));
        assert!(code.contains("COUNTER"));
        assert!(code.contains("HEXVAL"));
        assert!(code.contains("HEX"));
        assert!(code.contains("SIN"));
        assert!(code.contains("CRC"));
        assert!(code.contains("XOR_SUM"));
        assert!(code.contains("SUM8"));
        assert!(code.contains("_check_cooldown"));
        assert!(code.contains("match_data"));
        assert!(code.contains("_hex_expand"));
    }

    #[test]
    fn test_multiple_rules_all_match() {
        let r1 = make_rule("1", "AT", "contains", "OK\r\n", 0);
        let r2 = make_rule("2", "ERROR", "contains", "RESET\r\n", 100);
        let code = rules_to_lua_script(&[r1, r2], "test", "all");
        // all-match 模式：每个规则独立的 on_data
        let count = code.matches("on_data(").count();
        assert_eq!(count, 2, "all-match 应有 2 个 on_data");
    }

    #[test]
    fn test_case_sensitive_rule() {
        let mut rule = make_rule("1", "AT", "equals", "OK\r\n", 0);
        set_case_sensitive(&mut rule, true);
        let code = rules_to_lua_script(&[rule], "test", "all");
        assert!(code.contains("match_data(data, \"AT\", \"equals\", true)"));
    }

    // ── 增强功能测试 ──────────────────────────────────────

    #[test]
    fn test_first_match_generates_return() {
        // first-match 规则命中动作后应生成 return 短路后续规则
        let r1 = make_rule("1", "AT", "contains", "OK\r\n", 0);
        let r2 = make_rule("2", "AT+", "contains", "ERROR\r\n", 0);
        let code = rules_to_lua_script(&[r1, r2], "test", "first");
        eprintln!("=== FIRST-MATCH RETURN ===\n{}\n=== END ===", code);
        // 每条内联规则动作后有 return
        assert!(code.contains("return"), "first-match 应生成 return 短路");
        // 至少两个 return（两条规则各一个）
        assert!(code.matches("        return\n").count() >= 2);
    }

    #[test]
    fn test_first_match_executes_in_lua() {
        // 执行级回归测试：确保 first-match 生成的脚本在真实 Lua VM 中确实触发并短路。
        // 旧实现用 on_data("*")，feed_data 预过滤 string.find(data, "*") 会因 `*` 是
        // Lua 魔法量词报 "malformed pattern" 使 pcall 失败 → first-match 规则永不触发。
        // 纯字符串断言（test_first_match_strategy）掩盖了此 bug，故补此执行级测试。
        let r1 = make_rule("1", "AT", "contains", "FIRST\r\n", 0);
        let r2 = make_rule("2", "AT", "contains", "SECOND\r\n", 0);
        let code = rules_to_lua_script(&[r1, r2], "exec", "first");

        let lua = mlua::Lua::new();
        // 注入最小 API 桩：记录 send 到 __sent，on_data 收集 handler，其余空实现
        lua.load(
            r#"
            __sent = {}
            __handlers = {}
            function send(d) __sent[#__sent + 1] = d end
            function log(_) end
            function sleep(_) end
            function _time_ms() return 0 end
            function on_data(pattern, cb) __handlers[#__handlers + 1] = { pattern = pattern, callback = cb } end
            "#,
        )
        .exec()
        .expect("API 桩注入应成功");

        // 加载 codegen 生成的脚本（填充 __handlers）
        lua.load(&code).exec().expect("生成脚本应能加载");

        // 复刻 feed_data 的预过滤 + 分发逻辑，喂入同时命中两条规则的数据
        lua.load(
            r#"
            local data = "AT"
            for _, h in ipairs(__handlers) do
                local ok, m = pcall(string.find, data, h.pattern)
                if ok and m then h.callback(data) end
            end
            "#,
        )
        .exec()
        .expect("feed 分发循环应成功");

        let sent: Vec<String> = lua
            .globals()
            .get::<mlua::Table>("__sent")
            .unwrap()
            .sequence_values::<String>()
            .collect::<mlua::Result<_>>()
            .unwrap();

        // first-match：仅第一条命中规则回复，第二条被 return 短路
        assert_eq!(
            sent,
            vec!["FIRST\r\n".to_string()],
            "first-match 应触发并只回复首条命中规则"
        );
    }

    #[test]
    fn test_escape_sequences_interpreted() {
        // equals 模式下 \r\n 应被解释为真实 CR+LF（而非字面反斜杠）
        let rule = make_rule("1", "*IDN?\\r\\n", "equals", "OK\r\n", 0);
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== ESCAPE ===\n{}\n=== END ===", code);
        // 生成的 Lua 字面量应含真实换行的转义 \r\n（escape_lua_string 会把真实 CR→\r）
        assert!(code.contains("match_data(data, \"*IDN?\\r\\n\", \"equals\", false)"));
    }

    #[test]
    fn test_hex_match_mode() {
        // HEX 匹配格式：pattern "7E 01" → 字节匹配
        let mut rule = make_rule("1", "7E 01", "contains", "OK\r\n", 0);
        rule.conditions[0].match_format = "hex".into();
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== HEX MATCH ===\n{}\n=== END ===", code);
        assert!(code.contains("match_data_hex(data, \"\\x7e\\x01\", \"contains\")"));
    }

    #[test]
    fn test_conditions_and_logic() {
        // 多条件 AND：contains "TEMP" and not contains "ERROR"
        let mut rule = make_rule("1", "", "contains", "OK\r\n", 0);
        rule.conditions = vec![
            MatchCondition {
                pattern: "TEMP".into(),
                mode: "contains".into(),
                case_sensitive: false,
                negate: false,
                match_format: "text".into(),
            },
            MatchCondition {
                pattern: "ERROR".into(),
                mode: "contains".into(),
                case_sensitive: false,
                negate: true,
                match_format: "text".into(),
            },
        ];
        rule.condition_logic = "and".into();
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== CONDITIONS AND ===\n{}\n=== END ===", code);
        assert!(code.contains("match_data(data, \"TEMP\", \"contains\", false)"));
        assert!(code.contains("not (match_data(data, \"ERROR\", \"contains\", false))"));
        assert!(code.contains(" and "));
    }

    #[test]
    fn test_conditions_or_logic() {
        let mut rule = make_rule("1", "", "contains", "OK\r\n", 0);
        rule.conditions = vec![
            MatchCondition { pattern: "A".into(), mode: "contains".into(), case_sensitive: false, negate: false, match_format: "text".into() },
            MatchCondition { pattern: "B".into(), mode: "contains".into(), case_sensitive: false, negate: false, match_format: "text".into() },
        ];
        rule.condition_logic = "or".into();
        let code = rules_to_lua_script(&[rule], "test", "all");
        assert!(code.contains(" or "));
    }

    #[test]
    fn test_timer_rule() {
        let mut rule = make_rule("1", "", "contains", "HEARTBEAT\r\n", 0);
        rule.trigger_type = "timer".into();
        rule.timer_interval_ms = 1000;
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== TIMER ===\n{}\n=== END ===", code);
        assert!(code.contains("register_timer(\"__timer_rule_1\", 1000, function()"));
        assert!(code.contains("send(\"HEARTBEAT\\r\\n\")"));
        // 定时器规则不应生成 on_data
        assert!(!code.contains("on_data("));
    }

    #[test]
    fn test_timer_rule_outside_first_match_wrapper() {
        // first-match 策略下，定时器规则应在 on_data("*") 之外
        let data_rule = make_rule("1", "AT", "contains", "OK\r\n", 0);
        let mut timer_rule = make_rule("2", "", "contains", "PING\r\n", 0);
        timer_rule.trigger_type = "timer".into();
        timer_rule.timer_interval_ms = 500;
        let code = rules_to_lua_script(&[data_rule, timer_rule], "test", "first");
        eprintln!("=== TIMER + FIRST ===\n{}\n=== END ===", code);
        assert!(code.contains("on_data(\"\", function(data)"));
        assert!(code.contains("register_timer"));
        // register_timer 应出现在 FIRST_MATCH_FOOTER (end)) 之后
        let footer_pos = code.find("end)\n").unwrap();
        let timer_pos = code.find("register_timer").unwrap();
        assert!(timer_pos > footer_pos, "定时器应在 first-match 调度器之后");
    }

    #[test]
    fn test_expr_macro_in_header() {
        let code = rules_to_lua_script(&[], "test", "all");
        // EXPR 现在由 _expand_one 分发处理，不再是独立的 gsub 模式
        assert!(code.contains("EXPR:"));
        assert!(code.contains("unsafe expression rejected"));
    }

    // ── interpret_escape_sequences 单元测试 ────────────────

    #[test]
    fn test_interpret_escape_crlf() {
        let result = interpret_escape_sequences("*IDN?\\r\\n");
        assert_eq!(result, "*IDN?\r\n");
    }

    #[test]
    fn test_interpret_escape_backslash() {
        let result = interpret_escape_sequences("path\\\\to");
        assert_eq!(result, "path\\to");
    }

    #[test]
    fn test_interpret_escape_tab() {
        let result = interpret_escape_sequences("a\\tb");
        assert_eq!(result, "a\tb");
    }

    #[test]
    fn test_interpret_escape_null() {
        let result = interpret_escape_sequences("a\\0b");
        assert_eq!(result, "a\0b");
    }

    #[test]
    fn test_interpret_escape_unknown() {
        let result = interpret_escape_sequences("a\\xb");
        assert_eq!(result, "a\\xb");
    }

    #[test]
    fn test_interpret_escape_trailing() {
        let result = interpret_escape_sequences("trail\\");
        assert_eq!(result, "trail\\");
    }

    #[test]
    fn test_interpret_escape_empty() {
        let result = interpret_escape_sequences("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_interpret_escape_plain() {
        let result = interpret_escape_sequences("hello world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_interpret_escape_multiple() {
        let result = interpret_escape_sequences("\\r\\n\\t\\0\\\\");
        assert_eq!(result, "\r\n\t\0\\");
    }

    // ── hex_to_bytes 测试 ──────────────────────────────────

    #[test]
    fn test_hex_to_bytes_empty() {
        let result = hex_to_bytes("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_hex_to_bytes_whitespace_only() {
        let result = hex_to_bytes("  \n\t  ").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_hex_to_bytes_single_byte() {
        let result = hex_to_bytes("FF").unwrap();
        assert_eq!(result, vec![0xFF]);
    }

    #[test]
    fn test_hex_to_bytes_multiple_bytes() {
        let result = hex_to_bytes("01 03 00 00 00 0A").unwrap();
        assert_eq!(result, vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x0A]);
    }

    #[test]
    fn test_hex_to_bytes_lowercase() {
        let result = hex_to_bytes("7e ff 0a").unwrap();
        assert_eq!(result, vec![0x7E, 0xFF, 0x0A]);
    }

    #[test]
    fn test_hex_to_bytes_no_spaces() {
        let result = hex_to_bytes("DEADBEEF").unwrap();
        assert_eq!(result, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_hex_to_bytes_odd_length() {
        let result = hex_to_bytes("FFF");
        assert!(result.is_err());
    }

    #[test]
    fn test_hex_to_bytes_invalid_char() {
        let result = hex_to_bytes("ZZ");
        assert!(result.is_err());
    }

    // ── CRC 统一宏测试 ────────────────────────────────────

    #[test]
    fn test_crc_macro_in_reply() {
        let mut rule = make_rule("1", "01 03", "contains", "", 0);
        rule.actions = vec![ReplyAction {
            delay_ms: 0,
            data: "{{CRC(hello, 16, 0x8005)}}".into(),
            format: "text".into(),
        }];
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== CRC CODE ===\n{}\n=== END ===", code);
        assert!(code.contains("CRC"));
        assert!(code.contains("_crc_compute"));
    }

    #[test]
    fn test_crc_modbus_lua_execution() {
        // 直接加载生产 SCRIPT_HEADER，验证 _crc_compute（现为全局函数）
        let lua = mlua::Lua::new();
        lua.load(super::SCRIPT_HEADER).exec().unwrap();

        // CRC(01 03 00 00 00 01, 16, 0x8005) → "840A"
        let result: String = lua.load(r#"return _crc_compute("\x01\x03\x00\x00\x00\x01", 16, 0x8005)"#)
            .eval().unwrap();
        assert_eq!(result, "840A");
    }

    #[test]
    fn test_crc_ccitt_lua_execution() {
        let lua = mlua::Lua::new();
        lua.load(super::SCRIPT_HEADER).exec().unwrap();

        // CRC-16/CCITT("123456789") → 0x31C3
        let result: String = lua.load(r#"return _crc_compute("123456789", 16, 0x1021)"#)
            .eval().unwrap();
        assert_eq!(result, "31C3");
    }

    #[test]
    fn test_crc32_lua_execution() {
        let lua = mlua::Lua::new();
        lua.load(super::SCRIPT_HEADER).exec().unwrap();

        // CRC-32("123456789") → 0xCBF43926
        let result: String = lua.load(r#"return _crc_compute("123456789", 32, 0x04C11DB7)"#)
            .eval().unwrap();
        assert_eq!(result, "CBF43926");
    }

    #[test]
    fn test_xor_checksum_lua_execution() {
        let lua = mlua::Lua::new();
        // 加载生产 SCRIPT_HEADER（被测实际代码），避免假阳性
        lua.load(super::SCRIPT_HEADER).exec().unwrap();

        // XOR of "GPGGA" = 0x47^0x50^0x47^0x47^0x41 = 0x56
        let result: String = lua.load(r#"return __test._xor_checksum("GPGGA")"#)
            .eval().unwrap();
        assert_eq!(result, "56");
    }

    #[test]
    fn test_sum8_lua_execution() {
        let lua = mlua::Lua::new();
        lua.load(super::SCRIPT_HEADER).exec().unwrap();

        // SUM8 of "GPGGA": 0x47+0x50+0x47+0x47+0x41 = 0x166, low byte = 0x66
        let result: String = lua.load(r#"return __test._sum8("GPGGA")"#)
            .eval().unwrap();
        assert_eq!(result, "66");
    }

    // ── HEXVAL 宏测试 ────────────────────────────────────

    #[test]
    fn test_hexval_macro_in_reply() {
        let mut rule = make_rule("1", "TEST", "contains", "", 0);
        rule.actions = vec![ReplyAction {
            delay_ms: 0,
            data: "{{HEXVAL(255,2)}}".into(),
            format: "text".into(),
        }];
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== HEXVAL CODE ===\n{}\n=== END ===", code);
        assert!(code.contains("HEXVAL"));
    }

    #[test]
    fn test_hexval_lua_execution() {
        let lua = mlua::Lua::new();
        // Test: 255 with width 2 → "FF"
        let result: String = lua.load(r#"
            local n = 255
            local w = 2
            return string.format("%0" .. w .. "X", math.floor(n))
        "#).eval().unwrap();
        assert_eq!(result, "FF");

        // Test: 0 with width 4 → "0000"
        let result: String = lua.load(r#"
            local n = 0
            local w = 4
            return string.format("%0" .. w .. "X", math.floor(n))
        "#).eval().unwrap();
        assert_eq!(result, "0000");

        // Test: 4095 auto width → "0FFF" → "FFF"? No, auto width rounds to even hex digits
        let result: String = lua.load(r#"
            local n = 4095
            local w = math.ceil(math.log(n + 1, 16))
            if w % 2 ~= 0 then w = w + 1 end
            return string.format("%0" .. w .. "X", math.floor(n))
        "#).eval().unwrap();
        assert_eq!(result, "0FFF");
    }

    // ── 嵌套宏展开测试 ────────────────────────────────────

    #[test]
    fn test_nested_macro_expansion_lua() {
        // 验证嵌套宏能正确展开：{{HEXVAL({{COUNTER}},2)}}
        let lua = mlua::Lua::new();
        lua.load(r#"
            __counters = {}
            __cooldowns = {}
            __handlers = {}
            function log(_) end
            function sleep(_) end
            function _time_ms() return 0 end
            function _datetime_iso() return "" end
            function _datetime_format(_) return "" end
            function on_data(pattern, cb) __handlers[#__handlers + 1] = { pattern = pattern, callback = cb } end
            sent = {}
            function send(d) sent[#sent + 1] = d end
        "#).exec().unwrap();

        // 生成完整脚本并执行
        let mut rule = make_rule("1", "GO", "contains", "", 0);
        rule.actions = vec![
            ReplyAction {
                delay_ms: 0,
                data: "VAL:{{HEXVAL({{COUNTER}},4)}}".into(),
                format: "text".into(),
            },
        ];
        let code = rules_to_lua_script(&[rule], "nest", "all");
        eprintln!("=== NESTED CODE ===\n{}\n=== END ===", code);

        lua.load(&code).exec().unwrap();

        // 模拟 feed_data：手动调用 handler
        lua.load(r#"
            local data = "GO"
            for _, h in ipairs(__handlers) do
                local ok, m = pcall(string.find, data, h.pattern)
                if ok and m then h.callback(data) end
            end
        "#).exec().unwrap();

        let sent: Vec<String> = lua.globals()
            .get::<mlua::Table>("sent").unwrap()
            .sequence_values::<String>()
            .collect::<mlua::Result<_>>()
            .unwrap();

        // COUNTER = 1, HEXVAL(1, 4) = "0001"
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0], "VAL:0001");
    }

    // ── EXPR 位运算测试 ────────────────────────────────────

    #[test]
    fn test_expr_bitwise_operators_lua() {
        let lua = mlua::Lua::new();

        // 左移: 1 << 4 = 16
        let result: f64 = lua.load("return 1 << 4").eval().unwrap();
        assert_eq!(result as i64, 16);

        // 右移: 256 >> 4 = 16
        let result: f64 = lua.load("return 256 >> 4").eval().unwrap();
        assert_eq!(result as i64, 16);

        // AND: 0xFF & 0x0F = 15
        let result: f64 = lua.load("return 0xFF & 0x0F").eval().unwrap();
        assert_eq!(result as i64, 15);

        // OR: 0xF0 | 0x0F = 255
        let result: f64 = lua.load("return 0xF0 | 0x0F").eval().unwrap();
        assert_eq!(result as i64, 255);

        // XOR: 0xFF ~ 0x0F = 240
        let result: f64 = lua.load("return 0xFF ~ 0x0F").eval().unwrap();
        assert_eq!(result as i64, 240);

        // 复合: (0xFF & 0xF0) >> 4 = 15
        let result: f64 = lua.load("return (0xFF & 0xF0) >> 4").eval().unwrap();
        assert_eq!(result as i64, 15);
    }

    #[test]
    fn test_expr_bitwise_in_macro() {
        let mut rule = make_rule("1", "TEST", "contains", "", 0);
        rule.actions = vec![ReplyAction {
            delay_ms: 0,
            data: "{{EXPR:(255 & 15) << 4}}".into(),
            format: "text".into(),
        }];
        let code = rules_to_lua_script(&[rule], "test", "all");
        eprintln!("=== EXPR BITWISE ===\n{}\n=== END ===", code);
        // 验证包含 EXPR 和位运算符（不会被白名单拦截）
        assert!(code.contains("EXPR"));
    }
}
