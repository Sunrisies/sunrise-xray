use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use std::time::Duration;

/// 拉取订阅并返回解码后的文本（按行排列的 vless:// / hysteria2:// 等 URI）。
pub async fn fetch_subscription(url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent("sunrise-xray/0.1")
        .timeout(Duration::from_secs(30))
        .build()
        .context("构造 HTTP 客户端失败")?;

    let body = client
        .get(url)
        .send()
        .await
        .context("订阅请求发送失败")?
        .error_for_status()
        .context("订阅返回非 2xx")?
        .text()
        .await
        .context("读取订阅响应失败")?;

    // 有的订阅会直接返回明文 URI 列表；多数会返回 base64。
    if body.contains("://") {
        Ok(body)
    } else {
        decode_base64_loose(&body).context("订阅 base64 解码失败")
    }
}

fn decode_base64_loose(s: &str) -> Result<String> {
    let mut cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    // URL-safe 兼容
    cleaned = cleaned.replace('-', "+").replace('_', "/");
    while cleaned.len() % 4 != 0 {
        cleaned.push('=');
    }
    let bytes = general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .context("base64 decode 失败")?;
    String::from_utf8(bytes).context("base64 解码后的字节不是合法 UTF-8")
}
