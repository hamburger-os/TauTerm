//! 字符编码转码
//!
//! - **发送方向**：前端文本路径（SendBar / 键盘 / Lua `send_text`）将 UTF-8
//!   字节交由 `transcode_utf8_to_encoding` 按会话编码转码后写入设备；HEX 发送
//!   与 Lua `send`（原始字节）路径原样透传，不做转码。
//! - **接收方向**：日志写入将设备原始字节按会话编码解码回 UTF-8
//!   （`decode_to_utf8`），保证文本格式日志恒为可读 UTF-8。

use encoding_rs::EncoderResult;

/// 按标签解析编码器（大小写不敏感、下划线等价连字符，与 WHATWG 标签
/// 匹配语义对齐 —— 如 `Shift_JIS` 规范名 ↔ 列表 `shift-jis`）。
///
/// 支持标签与前端 `src/utils/charsets.ts` 的 CHARSETS 保持同步——
/// 新增编码需同时更新前端列表与本函数，否则未知标签原样透传。
///
/// 未知标签返回 `None` — 调用方应原样透传，避免静默按错误编码转码。
pub fn resolve_encoder(encoding: &str) -> Option<&'static encoding_rs::Encoding> {
    let normalized = encoding.to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        "gbk" => Some(encoding_rs::GBK),
        "gb18030" => Some(encoding_rs::GB18030),
        "big5" => Some(encoding_rs::BIG5),
        "shift-jis" => Some(encoding_rs::SHIFT_JIS),
        "euc-jp" => Some(encoding_rs::EUC_JP),
        "euc-kr" => Some(encoding_rs::EUC_KR),
        // WHATWG 中 "iso-8859-1" 标签映射到 windows-1252 编码器
        "iso-8859-1" => Some(encoding_rs::WINDOWS_1252),
        _ => None,
    }
}

/// 将 UTF-8 文本转码为目标字符编码（发送方向）。
///
/// 返回 `None`：编码未知，或输入非 UTF-8（调用方误用 — transcode 仅应
/// 用于文本路径）。不可映射的字符输出替换字节 `?`（0x3F），而非 WHATWG
/// 的 numeric character reference（`&#xNNNN;` 字面量会污染终端设备显示）。
pub fn transcode_utf8_to_encoding(data: &[u8], encoding: &str) -> Option<Vec<u8>> {
    let encoder = resolve_encoder(encoding)?;
    // 输入由前端 `TextEncoder` 产生，应为合法 UTF-8；非 UTF-8 输入拒绝转码
    let text = std::str::from_utf8(data).ok()?;
    if text.is_empty() {
        return Some(Vec::new());
    }

    // 增量编码：编码器在不可映射字符处返回 Unmappable(char)（不输出 NCR），
    // 将该字符替换为 '?' 后继续。列表内编码均为无状态编码，last 传 true 无行为差异。
    let mut enc = encoder.new_encoder();
    let mut out = Vec::with_capacity(data.len());
    let mut rest = text.as_bytes();
    let mut buf = [0u8; 64];
    loop {
        if rest.is_empty() {
            break;
        }
        // rest 始终是合法 UTF-8 文本的后缀切片
        let s = unsafe { std::str::from_utf8_unchecked(rest) };
        match enc.encode_from_utf8_without_replacement(s, &mut buf, true) {
            (EncoderResult::InputEmpty, _, written) => {
                out.extend_from_slice(&buf[..written]);
                break;
            }
            (EncoderResult::Unmappable(_ch), read, written) => {
                out.extend_from_slice(&buf[..written]);
                // read 已包含失败字符本身（encoding_rs 语义：返回的 read
                // 指向失败字符之后），直接跳过，输出替换字节 '?'
                rest = &rest[read..];
                out.push(b'?');
            }
            (EncoderResult::OutputFull, read, written) => {
                out.extend_from_slice(&buf[..written]);
                rest = &rest[read..];
            }
        }
    }
    Some(out)
}

/// 将设备原始字节按会话编码解码为 UTF-8 文本（日志写入方向）。
///
/// 返回 `None`：编码未知——调用方应回退 `from_utf8_lossy`（等价旧行为）。
/// 无效字节序列按 encoding_rs 语义替换为 U+FFFD（与 WHATWG 解码器一致）。
pub fn decode_to_utf8(data: &[u8], encoding: &str) -> Option<String> {
    if encoding.eq_ignore_ascii_case("utf-8") {
        return Some(String::from_utf8_lossy(data).into_owned());
    }
    let enc = resolve_encoder(encoding)?;
    let (text, _) = enc.decode_without_bom_handling(data);
    Some(text.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gbk_transcode_chinese() {
        // "你好" UTF-8 = E4 BD A0 E5 A5 BD → GBK = C4 E3 BA C3
        let out = transcode_utf8_to_encoding("你好".as_bytes(), "gbk").unwrap();
        assert_eq!(out, vec![0xC4, 0xE3, 0xBA, 0xC3]);
    }

    #[test]
    fn unmappable_char_replaced_with_question_mark() {
        // "世" (U+4E16) 在 iso-8859-1（windows-1252）中无映射：
        // 应输出 '?'（0x3F），而非 WHATWG 的 NCR 文本
        let out = transcode_utf8_to_encoding("世".as_bytes(), "iso-8859-1").unwrap();
        assert_eq!(out, vec![b'?']);
        // 可映射字符保持原样（"A" 在 ASCII 区）
        let out = transcode_utf8_to_encoding("A".as_bytes(), "iso-8859-1").unwrap();
        assert_eq!(out, vec![b'A']);
    }

    #[test]
    fn encoding_label_case_insensitive() {
        // 与 JS TextDecoder 的大小写不敏感标签匹配语义对齐
        let lower = transcode_utf8_to_encoding("你好".as_bytes(), "gbk").unwrap();
        let upper = transcode_utf8_to_encoding("你好".as_bytes(), "GBK").unwrap();
        let mixed = transcode_utf8_to_encoding("你好".as_bytes(), "Shift_JIS").unwrap();
        assert_eq!(lower, upper);
        assert_eq!(mixed, transcode_utf8_to_encoding("你好".as_bytes(), "shift-jis").unwrap());
    }

    #[test]
    fn unknown_encoding_and_utf8_return_none() {
        // 未知编码（如旧版 gb2312）与 utf-8 均返回 None → 调用方原样透传
        assert!(transcode_utf8_to_encoding(b"abc", "gb2312").is_none());
        assert!(transcode_utf8_to_encoding(b"abc", "utf-8").is_none());
    }

    #[test]
    fn non_utf8_input_returns_none() {
        // 非 UTF-8 字节（transcode 误用于二进制）拒绝转码，不静默损坏
        assert!(transcode_utf8_to_encoding(&[0xFF, 0xFE, 0x01], "gbk").is_none());
    }

    #[test]
    fn mixed_mappable_and_unmappable() {
        // 混合输入验证增量循环：不可映射字符前后均有可映射内容
        let out = transcode_utf8_to_encoding("A世B".as_bytes(), "iso-8859-1").unwrap();
        assert_eq!(out, b"A?B");
    }

    #[test]
    fn shift_jis_roundtrip() {
        // Shift-JIS 多字节：ア (U+30A2) = 83 41
        let out = transcode_utf8_to_encoding("ア".as_bytes(), "shift-jis").unwrap();
        assert_eq!(out, vec![0x83, 0x41]);
    }

    #[test]
    fn decode_gbk_roundtrip() {
        // GBK 字节 C4 E3 BA C3 → "你好"
        let out = decode_to_utf8(&[0xC4, 0xE3, 0xBA, 0xC3], "gbk").unwrap();
        assert_eq!(out, "你好");
    }

    #[test]
    fn decode_utf8_passthrough() {
        // utf-8 标签：等价 from_utf8_lossy（含无效序列 → U+FFFD）
        assert_eq!(decode_to_utf8("中文".as_bytes(), "utf-8").unwrap(), "中文");
        let lossy = decode_to_utf8(&[0xFF, 0xFE, 0x01], "utf-8").unwrap();
        assert_eq!(lossy, "\u{FFFD}\u{FFFD}\u{0001}");
    }

    #[test]
    fn decode_invalid_bytes_replaced() {
        // GBK 中的无效字节（0x00 非 GBK 首字节）→ U+FFFD 替换，不 panic
        let out = decode_to_utf8(&[0xC4, 0x00], "gbk").unwrap();
        assert!(out.contains('\u{FFFD}'));
    }

    #[test]
    fn decode_unknown_encoding_returns_none() {
        assert!(decode_to_utf8(b"abc", "not-a-charset").is_none());
    }

    #[test]
    fn decode_big5_and_shift_jis() {
        // Big5 多字节：中 (U+4E2D) = A4 A4
        assert_eq!(decode_to_utf8(&[0xA4, 0xA4], "big5").unwrap(), "中");
        // Shift-JIS：ア (U+30A2) = 83 41
        assert_eq!(decode_to_utf8(&[0x83, 0x41], "shift-jis").unwrap(), "ア");
    }

    #[test]
    fn encode_decode_roundtrip_non_ascii() {
        // 发送方向转码 → 接收方向解码，双向一致。
        // 每种编码使用其保证可映射的字符集（GBK 覆盖 GB2312 全角标点）。
        let cases = [
            ("gbk", "你好，TauTerm！"),
            ("big5", "中文字"),
            ("shift-jis", "アテスト"),
            ("euc-kr", "한국"),
        ];
        for (enc, text) in cases {
            let bytes = transcode_utf8_to_encoding(text.as_bytes(), enc).unwrap();
            assert_eq!(decode_to_utf8(&bytes, enc).unwrap(), text, "{}", enc);
        }
    }
}
