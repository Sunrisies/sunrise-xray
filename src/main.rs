mod config;
mod fetch;
mod xray;

use anyhow::{anyhow, Context, Result};
use std::process::ExitCode;

const SUB_URL_ENV: &str = "SUNRISE_SUB_URL";
const SOCKS_PORT: u16 = 10808;
const HTTP_PORT: u16 = 10809;
const CONFIG_PATH: &str = "/tmp/xray_config.json";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\n[错误] {e:#}");
            ExitCode::from(1)
        }
    }
}

async fn run() -> Result<()> {
    let sub_url = std::env::var(SUB_URL_ENV).map_err(|_| {
        anyhow!("环境变量 {SUB_URL_ENV} 未设置；请设置订阅地址后再运行")
    })?;

    println!("[1/4] 拉取订阅...");
    let raw = fetch::fetch_subscription(&sub_url).await?;

    println!("[2/4] 解析订阅，挑选 VLESS+REALITY 节点...");
    let node = config::pick_reality_node(&raw)?;
    println!(
        "       使用节点: {}  ({}:{})",
        node.name, node.address, node.port
    );

    println!("[3/4] 生成本地配置: {CONFIG_PATH}");
    let local = config::build_local_config(&node.outbound, SOCKS_PORT, HTTP_PORT);
    let bytes = serde_json::to_vec_pretty(&local).context("序列化本地配置失败")?;
    tokio::fs::write(CONFIG_PATH, &bytes)
        .await
        .with_context(|| format!("写入配置文件失败: {CONFIG_PATH}"))?;

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

    xray::run_xray(&binary, CONFIG_PATH).await
}
