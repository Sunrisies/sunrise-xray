mod commands;
mod config;
mod embedded;
mod fetch;
mod paths;
mod util;
mod xray;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use std::process::ExitCode;
use url::Url;

const SUB_URL_ENV: &str = "SUNRISE_SUB_URL";
const DEFAULT_SOCKS_PORT: u16 = 10808;
const DEFAULT_HTTP_PORT: u16 = 10809;

/// 把订阅链接自动拉成本地 Xray 代理服务的小工具。
///
/// 订阅地址通过环境变量 SUNRISE_SUB_URL 传入。
/// 默认（不带子命令）以前台模式跑代理，Ctrl+C 停。
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// 选择节点：纯数字按索引（如 `--node 3`），其他按名字子串匹配（如 `--node 香港`）。
    /// 也可通过 SUNRISE_NODE 环境变量传入，命令行参数优先。
    #[arg(long, env = "SUNRISE_NODE", global = true)]
    node: Option<String>,

    /// 本地 SOCKS5 监听端口。
    #[arg(long, env = "SUNRISE_SOCKS_PORT", default_value_t = DEFAULT_SOCKS_PORT, global = true)]
    socks_port: u16,

    /// 本地 HTTP 监听端口。
    #[arg(long, env = "SUNRISE_HTTP_PORT", default_value_t = DEFAULT_HTTP_PORT, global = true)]
    http_port: u16,

    /// 列出订阅里所有可用节点后退出（兼容旧用法，等价于子命令 `list`）。
    #[arg(long)]
    list: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

/// proxy 子命令的子选项。
#[derive(Subcommand, Debug)]
enum ProxySub {
    /// 输出 export http_proxy / https_proxy / all_proxy 语句（eval 用）。
    On,
    /// 输出 unset http_proxy / https_proxy / all_proxy 语句（eval 用）。
    Off,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 后台启动代理（同义词: on）。
    #[command(alias = "on")]
    Start,

    /// 停止后台代理（同义词: off）。
    #[command(alias = "off")]
    Stop,

    /// 重启后台代理（stop + start）。
    Restart,

    /// 查看后台代理状态。
    Status,

    /// 通过代理测试几个站点（Google / GitHub / ipify 等）。
    Test,

    /// 查看后台代理日志。
    Logs {
        /// 显示最后 N 行。
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
        /// 持续跟踪新增内容（Ctrl+C 停）。
        #[arg(short = 'f', long)]
        follow: bool,
    },

    /// 列出订阅里所有可用节点（同义词: ls）。
    #[command(alias = "ls")]
    List,

    /// 交互式选择节点（测延迟、列表上下选、确认即切换；同义词: pick / switch）。
    #[command(alias = "pick", alias = "switch")]
    Use,

    /// 健康检查 + 自动故障转移。当前节点失活时自动切到延迟最低的活节点。
    /// 适合放在 crontab 里定时跑（健康时秒退，cron 友好）。
    Autoswitch,

    /// 输出 shell eval 兼容的代理环境变量（export / unset）。
    /// 搭配 eval 使用：eval "$(sunrise-xray proxy on)" 或 eval "$(sunrise-xray proxy off)"。
    #[command(subcommand)]
    Proxy(ProxySub),
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\n[错误] {e:#}");
            ExitCode::from(1)
        }
    }
}

async fn dispatch(cli: Cli) -> Result<()> {
    anyhow::ensure!(
        cli.socks_port != cli.http_port,
        "SOCKS 与 HTTP 端口不能相同（都设置为 {}）",
        cli.socks_port
    );
    anyhow::ensure!(cli.socks_port != 0, "SOCKS 端口不能为 0");
    anyhow::ensure!(cli.http_port != 0, "HTTP 端口不能为 0");

    // 子命令分派
    match (cli.list, &cli.command) {
        (true, None) | (false, Some(Command::List)) => return list_nodes().await,
        (true, Some(_)) => anyhow::bail!("--list 不能和子命令同时使用"),

        (false, Some(Command::Start)) => {
            return commands::cmd_start(cli.node.as_deref(), cli.socks_port, cli.http_port).await;
        }
        (false, Some(Command::Stop)) => return commands::cmd_stop(),
        (false, Some(Command::Restart)) => {
            return commands::cmd_restart(cli.node.as_deref(), cli.socks_port, cli.http_port).await;
        }
        (false, Some(Command::Status)) => return commands::cmd_status(),
        (false, Some(Command::Test)) => return commands::cmd_test(cli.http_port).await,
        (false, Some(Command::Logs { lines, follow })) => {
            return commands::cmd_logs(*lines, *follow).await;
        }
        (false, Some(Command::Use)) => {
            return commands::cmd_use(cli.socks_port, cli.http_port).await;
        }
        (false, Some(Command::Autoswitch)) => {
            return commands::cmd_autoswitch(cli.socks_port, cli.http_port).await;
        }
        (false, Some(Command::Proxy(sub))) => {
            return match sub {
                ProxySub::On => Ok(commands::cmd_proxy_on(cli.socks_port, cli.http_port)),
                ProxySub::Off => Ok(commands::cmd_proxy_off()),
            };
        }
        (false, None) => {} // fall through to foreground 默认行为
    }

    // 默认：旧的前台模式
    run_foreground(cli).await
}

async fn list_nodes() -> Result<()> {
    let raw = fetch_sub().await?;
    let nodes = config::parse_subscription(&raw);
    anyhow::ensure!(!nodes.is_empty(), "订阅里没有可用节点");
    config::print_node_list(&nodes, None);
    Ok(())
}

async fn fetch_sub() -> Result<String> {
    let sub_url_raw = std::env::var(SUB_URL_ENV)
        .map_err(|_| anyhow!("环境变量 {SUB_URL_ENV} 未设置；请设置订阅地址后再运行"))?;
    let sub_url = Url::parse(&sub_url_raw)
        .map_err(|e| anyhow!("{SUB_URL_ENV} 不是合法 URL: {e}"))?;
    anyhow::ensure!(
        matches!(sub_url.scheme(), "http" | "https"),
        "{SUB_URL_ENV} 必须是 http(s):// 开头，当前是: {}",
        sub_url.scheme()
    );
    println!("[1/4] 拉取订阅...");
    let raw = fetch::fetch_subscription(sub_url.as_str()).await?;
    println!("[2/4] 解析订阅...");
    Ok(raw)
}

async fn run_foreground(cli: Cli) -> Result<()> {
    let (binary, config_path, idx, node) =
        commands::prepare(cli.node.as_deref(), cli.socks_port, cli.http_port).await?;

    println!();
    println!("==============================================");
    println!("  代理已启动 [{}] {}", idx, node.name);
    println!("    SOCKS5  ->  socks5://127.0.0.1:{}", cli.socks_port);
    println!("    HTTP    ->  http://127.0.0.1:{}", cli.http_port);
    println!("  按 Ctrl+C 退出（后台运行请用 'sunrise-xray on'）");
    println!("==============================================");
    println!();

    xray::run_xray(&binary, &config_path).await
}
