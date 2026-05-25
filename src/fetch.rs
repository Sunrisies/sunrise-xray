use crate::util;
use anyhow::{anyhow, Context, Result};
use std::time::Duration;

/// 拉取订阅并返回解码后的文本（按行排列的 vless:// / trojan:// 等 URI）。
pub async fn fetch_subscription(url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("sunrise-xray/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .context("构造 HTTP 客户端失败")?;

    // reqwest 的错误链里会包含完整请求 URL；订阅 URL 通常带 token，
    // 不能让它进日志。统一包一层把 URL 字串脱敏掉。
    let scrub = |e: reqwest::Error| -> anyhow::Error {
        anyhow!("{}", util::redact_url_in(&e.to_string(), url))
    };

    let body = client
        .get(url)
        .send()
        .await
        .map_err(scrub)
        .context("订阅请求发送失败")?
        .error_for_status()
        .map_err(scrub)
        .context("订阅返回非 2xx")?
        .text()
        .await
        .map_err(scrub)
        .context("读取订阅响应失败")?;

    // 有的订阅会直接返回明文 URI 列表；多数会返回 base64。
    if body.contains("://") {
        Ok(body)
    } else {
        util::decode_base64_loose(&body).context("订阅 base64 解码失败")
    }
}
