//! System log redaction boundary.
//!
//! System events are free-form diagnostics, so the sink applies a final defensive scrub before
//! persistence. Callers must still avoid formatting credentials in the first place; this module is
//! a last line of defence, not a credential transport mechanism.

use regex::Regex;
use std::sync::LazyLock;

static JSON_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)("(?:password|passphrase|secret|private_key|token|access_token|refresh_token|api_key|authorization|cookie|set-cookie)"\s*:\s*)"(?:\\.|[^"\\])*""#,
    )
    .expect("valid system log JSON-secret regex")
});

static ASSIGNMENT_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(password|passphrase|secret|private_key|token|access_token|refresh_token|api_key|authorization|cookie)\b\s*=\s*[^\s,;]+",
    )
    .expect("valid system log assignment-secret regex")
});

static AUTH_SCHEME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+")
        .expect("valid system log authorization regex")
});

static PRIVATE_KEY_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)-----BEGIN [^-\r\n]*PRIVATE KEY-----.*?-----END [^-\r\n]*PRIVATE KEY-----",
    )
    .expect("valid private-key block regex")
});

/// Redact common credential forms and force one physical line per event.
///
/// Escaping CR/LF prevents an untrusted frontend event or remote error string from forging extra
/// system-log records while keeping the original control characters visible to diagnostics.
pub fn sanitize_log(message: &str) -> String {
    let mut result = PRIVATE_KEY_BLOCK
        .replace_all(message, "[REDACTED PRIVATE KEY]")
        .into_owned();
    result = JSON_SECRET
        .replace_all(&result, "${1}\"[REDACTED]\"")
        .into_owned();
    result = ASSIGNMENT_SECRET
        .replace_all(&result, "${1}=[REDACTED]")
        .into_owned();
    result = AUTH_SCHEME
        .replace_all(&result, "${1} [REDACTED]")
        .into_owned();
    result.replace('\r', "\\r").replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_json_credentials_case_insensitively() {
        let input = r#"{"username":"root","PASSWORD":"secret123","access_token":"abc.def"}"#;
        let output = sanitize_log(input);
        assert!(!output.contains("secret123"));
        assert!(!output.contains("abc.def"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_assignment_and_authorization_credentials() {
        let input = "api_key=abc123 Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.token.payload";
        let output = sanitize_log(input);
        assert!(!output.contains("abc123"));
        assert!(!output.contains("eyJhbGci"));
        assert!(output.contains("api_key=[REDACTED]"));
        assert!(output.contains("Bearer [REDACTED]"));
    }

    #[test]
    fn redacts_private_key_blocks() {
        let input = "key=-----BEGIN OPENSSH PRIVATE KEY-----\nsecret\n-----END OPENSSH PRIVATE KEY-----";
        let output = sanitize_log(input);
        assert!(!output.contains("secret"));
        assert!(output.contains("[REDACTED PRIVATE KEY]"));
    }

    #[test]
    fn escapes_record_injection_newlines() {
        let output = sanitize_log("connected\n[ERROR] forged");
        assert_eq!(output, "connected\\n[ERROR] forged");
    }

    #[test]
    fn non_sensitive_messages_pass_through() {
        let input = "Connected to COM3 at 115200 baud";
        assert_eq!(sanitize_log(input), input);
    }
}
