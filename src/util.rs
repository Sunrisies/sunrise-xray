use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};

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
}
