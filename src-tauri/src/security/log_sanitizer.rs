//! 日志脱敏
//!
//! 自动过滤日志中的密码、密钥、Token 等敏感信息。

/// 脱敏日志消息中的敏感信息
/// 在 LogEngine 消费者线程中自动应用于所有系统事件日志。
pub fn sanitize_log(message: &str) -> String {
    let mut result = message.to_string();

    // 过滤 "password": "..." 模式（JSON 格式）
    result = sanitize_json_field(&result, "password");
    result = sanitize_json_field(&result, "passphrase");
    result = sanitize_json_field(&result, "secret");
    result = sanitize_json_field(&result, "private_key");

    // 过滤 PRIVATE KEY 块
    if let Some(start) = result.find("-----BEGIN") {
        if let Some(end) = result.rfind("-----") {
            if end > start {
                let end_idx = end + 5;
                if end_idx <= result.len() {
                    result.replace_range(start..end_idx, "[REDACTED PRIVATE KEY]");
                }
            }
        }
    }

    // 过滤 Bearer token
    if let Some(start) = result.find("Bearer ") {
        let after = &result[start + 7..];
        if let Some(end) = after.find(|c: char| c.is_whitespace() || c == ',') {
            result.replace_range(start..start + 7 + end, "Bearer [REDACTED]");
        } else {
            result.replace_range(start.., "Bearer [REDACTED]");
        }
    }

    result
}

fn sanitize_json_field(text: &str, field: &str) -> String {
    let pattern = format!("\"{}\"", field);
    let mut result = text.to_string();
    let mut search_from = 0;

    while let Some(pos) = result[search_from..].find(&pattern) {
        let abs_pos = search_from + pos;
        // 查找后面的 ": "
        if let Some(colon_pos) = result[abs_pos..].find(':') {
            let value_start = abs_pos + colon_pos + 1;
            let after_colon = &result[value_start..];
            // 跳过空白
            let trimmed = after_colon.trim_start();
            let trim_offset = after_colon.len() - trimmed.len();
            let actual_start = value_start + trim_offset;

            if let Some(stripped) = trimmed.strip_prefix('"') {
                // 查找闭合引号
                if let Some(close_quote) = stripped.find('"') {
                    let end = actual_start + 1 + close_quote + 1;
                    result.replace_range(actual_start..end, "\"[REDACTED]\"");
                    search_from = actual_start + 12;
                } else {
                    search_from = abs_pos + 1;
                }
            } else {
                search_from = abs_pos + 1;
            }
        } else {
            search_from = abs_pos + 1;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_password_field() {
        let input = r#"{"username": "root", "password": "secret123"}"#;
        let output = sanitize_log(input);
        assert!(!output.contains("secret123"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn test_sanitize_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.token.payload";
        let output = sanitize_log(input);
        assert!(!output.contains("eyJhbGci"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn test_non_sensitive_passes_through() {
        let input = "Connected to COM3 at 115200 baud";
        let output = sanitize_log(input);
        assert_eq!(output, input);
    }
}
