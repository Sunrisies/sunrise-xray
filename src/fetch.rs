use crate::util;
use anyhow::{Context, Result};
use std::time::Duration;

/// 拉取订阅并返回解码后的文本（按行排列的 vless:// / trojan:// 等 URI）。
pub async fn fetch_subscription(url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("sunrise-xray/", env!("CARGO_PKG_VERSION")))
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
        util::decode_base64_loose(&body).context("订阅 base64 解码失败")
    }
}
