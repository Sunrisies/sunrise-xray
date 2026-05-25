use crate::util;
use anyhow::{anyhow, Context, Result};
use std::time::Duration;

/// 最多尝试次数（含初次请求）。指数退避：第 N 次重试前等 2^(N-1) 秒。
/// MAX_ATTEMPTS = 3 → 等待序列 0s / 1s / 3s，最坏 ~94s 后报错。
const MAX_ATTEMPTS: u32 = 3;

/// 拉取订阅并返回解码后的文本（按行排列的 vless:// / trojan:// 等 URI）。
///
/// 网络层失败 / 5xx / 429 会做指数退避重试；其它 4xx 立即失败（重试无用）。
pub async fn fetch_subscription(url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("sunrise-xray/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .context("构造 HTTP 客户端失败")?;

    let scrub = |s: String| util::redact_url_in(&s, url);
    let mut last_err: Option<String> = None;

    for attempt in 1..=MAX_ATTEMPTS {
        if attempt > 1 {
            let backoff = Duration::from_secs(1u64 << (attempt - 2)); // 1s, 2s, 4s...
            eprintln!(
                "       等 {}s 后做第 {} 次尝试 (上次原因: {})...",
                backoff.as_secs(),
                attempt,
                last_err.as_deref().unwrap_or("?")
            );
            tokio::time::sleep(backoff).await;
        }

        match fetch_once(&client, url).await {
            Ok(body) => {
                if body.contains("://") {
                    return Ok(body);
                }
                return util::decode_base64_loose(&body).context("订阅 base64 解码失败");
            }
            Err(e) => {
                let msg = scrub(e.message);
                if !e.retryable {
                    // 4xx（非 429）等：再试也是徒劳，立即失败
                    return Err(anyhow!("订阅请求失败（不可重试）: {}", msg));
                }
                last_err = Some(msg);
            }
        }
    }

    Err(anyhow!(
        "订阅请求失败（已尝试 {} 次）: {}",
        MAX_ATTEMPTS,
        last_err.unwrap_or_else(|| "未知原因".into())
    ))
}

/// 单次请求的结果。retryable=false 表示这是个"再试也没意义"的错（典型 4xx）。
struct AttemptError {
    retryable: bool,
    message: String,
}

async fn fetch_once(client: &reqwest::Client, url: &str) -> std::result::Result<String, AttemptError> {
    let resp = client.get(url).send().await.map_err(|e| AttemptError {
        // 传输层错误（timeout / connect refused / DNS）几乎都是瞬时
        retryable: true,
        message: e.to_string(),
    })?;

    let status = resp.status();
    if status.is_client_error() && status.as_u16() != 429 {
        // 401/403/404 等：URL 写错 / 订阅过期 / 权限错；重试无用
        let body_preview = resp
            .text()
            .await
            .ok()
            .map(|s| s.chars().take(120).collect::<String>())
            .unwrap_or_default();
        return Err(AttemptError {
            retryable: false,
            message: format!("HTTP {} (body: {})", status, body_preview),
        });
    }

    // 5xx / 429 通过 error_for_status 转成 reqwest::Error，标记可重试
    let resp = resp.error_for_status().map_err(|e| AttemptError {
        retryable: true,
        message: e.to_string(),
    })?;

    resp.text().await.map_err(|e| AttemptError {
        retryable: true,
        message: e.to_string(),
    })
}
