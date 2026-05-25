use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};

/// 把订阅 URL 脱敏成 `scheme://host[:port]/***`，路径/查询/锚点全部丢弃。
/// 用于错误日志、用户分享调试输出时不暴露订阅 token。
/// 解析失败返回固定占位符，不回显原文。
pub fn redact_url(s: &str) -> String {
    match url::Url::parse(s) {
        Ok(u) => {
            let scheme = u.scheme();
            let host = u.host_str().unwrap_or("?");
            let port = u
                .port()
                .map(|p| format!(":{}", p))
                .unwrap_or_default();
            format!("{}://{}{}/***", scheme, host, port)
        }
        Err(_) => "<invalid-url>".to_string(),
    }
}

/// 在任意字符串里把已知的订阅 URL 替换成脱敏版本。reqwest 等库的错误
/// 链会把请求的 URL 嵌在 message 里，我们事后扫一遍替换掉。
pub fn redact_url_in(message: &str, original: &str) -> String {
    if original.is_empty() {
        return message.to_string();
    }
    message.replace(original, &redact_url(original))
}

/// 容错版 base64 解码：
/// - 自动剥离空白字符
/// - URL-safe 字母（`-` / `_`）会被转回标准字母（`+` / `/`）
/// - 缺失的 `=` padding 会被补齐
///
/// 适用场景：订阅 body、Shadowsocks SIP002 userinfo、VMess 的整段 JSON 等
/// 都常见这些"宽松"的 base64 变体。
pub fn decode_base64_loose(s: &str) -> Result<String> {
    let mut cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    cleaned = cleaned.replace('-', "+").replace('_', "/");
    while cleaned.len() % 4 != 0 {
        cleaned.push('=');
    }
    let bytes = general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .context("base64 decode 失败")?;
    String::from_utf8(bytes).context("base64 解码后的字节不是合法 UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_standard_base64() {
        assert_eq!(decode_base64_loose("aGVsbG8=").unwrap(), "hello");
    }

    #[test]
    fn decodes_unpadded_base64() {
        assert_eq!(decode_base64_loose("aGVsbG8").unwrap(), "hello");
    }

    #[test]
    fn decodes_url_safe_base64() {
        // `?` and `>` chars encode to URL-safe forms
        let original = "??>";
        let url_safe = "Pz8-"; // standard "Pz8+" with + → -
        assert_eq!(decode_base64_loose(url_safe).unwrap(), original);
    }

    #[test]
    fn strips_whitespace() {
        assert_eq!(decode_base64_loose("aGVs bG8=\n").unwrap(), "hello");
    }

    #[test]
    fn errors_on_invalid_base64() {
        assert!(decode_base64_loose("!!!").is_err());
    }

    #[test]
    fn redacts_typical_subscription_url() {
        assert_eq!(
            redact_url("https://sub.example.com/api/v1/user/abc123def456?token=xyz789"),
            "https://sub.example.com/***"
        );
    }

    #[test]
    fn redacts_url_with_port() {
        assert_eq!(
            redact_url("https://sub.example.com:8443/secret-token"),
            "https://sub.example.com:8443/***"
        );
    }

    #[test]
    fn redacts_url_with_no_path() {
        // URL crate normalizes "https://host" → "https://host/"，所以仍然显示 /***
        assert_eq!(
            redact_url("https://example.com"),
            "https://example.com/***"
        );
    }

    #[test]
    fn redacts_invalid_url_with_placeholder_not_original() {
        let bad = "not-a-url-secret-token";
        let out = redact_url(bad);
        assert_eq!(out, "<invalid-url>");
        // 关键：原文不能出现在脱敏结果里
        assert!(!out.contains("secret-token"));
    }

    #[test]
    fn redact_url_in_replaces_known_url_in_message() {
        let url = "https://sub.example.com/api/secret-token";
        let msg = format!("error sending request for url ({}): timeout", url);
        let scrubbed = redact_url_in(&msg, url);
        assert!(!scrubbed.contains("secret-token"));
        assert!(scrubbed.contains("https://sub.example.com/***"));
    }

    #[test]
    fn redact_url_in_noop_for_empty_original() {
        let msg = "some unrelated error";
        assert_eq!(redact_url_in(msg, ""), msg);
    }
}
