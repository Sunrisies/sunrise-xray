mod config;
mod fetch;
mod paths;
mod xray;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use std::process::ExitCode;
use url::Url;

const SUB_URL_ENV: &str = "SUNRISE_SUB_URL";
const SOCKS_PORT: u16 = 10808;
const HTTP_PORT: u16 = 10809;

/// 把订阅链接自动拉成本地 Xray 代理服务的小工具。
///
/// 订阅地址通过环境变量 SUNRISE_SUB_URL 传入。
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {}

#[tokio::main]
async fn main() -> ExitCode {
    let _cli = Cli::parse();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\n[错误] {e:#}");
            ExitCode::from(1)
        }
    }
}

async fn run() -> Result<()> {
    let sub_url_raw = std::env::var(SUB_URL_ENV)
        .map_err(|_| anyhow!("环境变量 {SUB_URL_ENV} 未设置；请设置订阅地址后再运行"))?;
    let sub_url = Url::parse(&sub_url_raw)
        .with_context(|| format!("{SUB_URL_ENV} 不是合法 URL: {sub_url_raw}"))?;
    anyhow::ensure!(
        matches!(sub_url.scheme(), "http" | "https"),
        "{SUB_URL_ENV} 必须是 http(s):// 开头，当前是: {}",
        sub_url.scheme()
    );

    println!("[1/4] 拉取订阅...");
    let raw = fetch::fetch_subscription(sub_url.as_str()).await?;

    println!("[2/4] 解析订阅，挑选 VLESS+REALITY 节点...");
    let node = config::pick_reality_node(&raw)?;
    println!(
        "       使用节点: {}  ({}:{})",
        node.name, node.address, node.port
    );

    let config_path = paths::xray_config_path()?;
    paths::ensure_parent(&config_path).await?;

    println!("[3/4] 生成本地配置: {}", config_path.display());
    let local = config::build_local_config(&node.outbound, SOCKS_PORT, HTTP_PORT);
    let bytes = serde_json::to_vec_pretty(&local).context("序列化本地配置失败")?;
    tokio::fs::write(&config_path, &bytes)
        .await
        .with_context(|| format!("写入配置文件失败: {}", config_path.display()))?;

    println!("[4/4] 准备 xray 可执行文件...");
    let binary = xray::ensure_xray().await?;
    println!("       使用 xray: {}", binary.display());

    println!();
    println!("==============================================");
    println!("  代理已启动，可用入口：");
    println!("    SOCKS5  ->  socks5://127.0.0.1:{SOCKS_PORT}");
    println!("    HTTP    ->  http://127.0.0.1:{HTTP_PORT}");
    println!("  按 Ctrl+C 退出");
    println!("==============================================");
    println!();

    xray::run_xray(&binary, &config_path).await
}
