//! 子命令实现：daemon 控制（on/off/restart/status）+ test/logs。
//!
//! 选择直接 spawn xray 子进程而不是把整个 sunrise-xray 自身 fork：
//! - xray-core 长期跑稳定，是真正的工作进程
//! - sunrise-xray 只做编排（订阅解析 + 配置生成），完成后退出即可
//! - 不需要做 double-fork daemon 那套，pre_exec(setsid) 已经够

use crate::{config, fetch, paths, xray};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 启动 daemon 前必须确认 sunrise-xray 没在跑（PID 文件指向的进程要么死了要么不存在）。
/// 在跑就返回 Some(pid)；没在跑就返回 None 并清掉过时的 PID 文件。
pub fn read_alive_pid() -> Result<Option<u32>> {
    let pid_path = paths::pid_path()?;
    if !pid_path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&pid_path)
        .with_context(|| format!("读 PID 文件失败: {}", pid_path.display()))?;
    let pid: u32 = match raw.trim().parse() {
        Ok(n) => n,
        Err(_) => {
            // 文件内容坏了，清掉
            let _ = std::fs::remove_file(&pid_path);
            return Ok(None);
        }
    };
    if process_alive(pid) {
        Ok(Some(pid))
    } else {
        // 过时的 PID 文件，清掉
        let _ = std::fs::remove_file(&pid_path);
        Ok(None)
    }
}

#[cfg(unix)]
pub fn process_alive(pid: u32) -> bool {
    // kill(pid, 0) 不发送信号，只检查目标存在性 + 当前用户有没有权限
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
pub fn process_alive(_pid: u32) -> bool {
    // Windows 下走不到这里（daemon 命令被屏蔽）
    false
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DaemonState {
    pub pid: u32,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub node_name: String,
    pub node_index: usize,
    pub node_protocol: String,
    pub socks_port: u16,
    pub http_port: u16,
}

fn write_state(state: &DaemonState) -> Result<()> {
    let path = paths::state_path()?;
    let bytes = serde_json::to_vec_pretty(state)?;
    std::fs::write(&path, bytes)
        .with_context(|| format!("写状态文件失败: {}", path.display()))?;
    Ok(())
}

fn read_state() -> Result<Option<DaemonState>> {
    let path = paths::state_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("读状态文件失败: {}", path.display()))?;
    Ok(serde_json::from_str(&raw).ok())
}

fn clear_runtime_files() -> Result<()> {
    let _ = std::fs::remove_file(paths::pid_path()?);
    let _ = std::fs::remove_file(paths::state_path()?);
    Ok(())
}

/// 共享主流程的"准备"阶段：拉订阅 → 选节点 → 写 xray 配置。
/// 返回 (xray 二进制路径, 选中的节点信息) — 后续步骤用得到。
pub async fn prepare(
    node_selector: Option<&str>,
    socks_port: u16,
    http_port: u16,
) -> Result<(PathBuf, PathBuf, usize, config::ProxyNode)> {
    let sub_url_raw = std::env::var("SUNRISE_SUB_URL")
        .map_err(|_| anyhow!("环境变量 SUNRISE_SUB_URL 未设置；请设置订阅地址后再运行"))?;
    let sub_url = url::Url::parse(&sub_url_raw)
        .with_context(|| format!("SUNRISE_SUB_URL 不是合法 URL: {sub_url_raw}"))?;
    anyhow::ensure!(
        matches!(sub_url.scheme(), "http" | "https"),
        "SUNRISE_SUB_URL 必须是 http(s):// 开头"
    );

    println!("[1/4] 拉取订阅...");
    let raw = fetch::fetch_subscription(sub_url.as_str()).await?;

    println!("[2/4] 解析订阅...");
    let nodes = config::parse_subscription(&raw);
    anyhow::ensure!(!nodes.is_empty(), "订阅里没有可用节点");

    let selector = node_selector
        .map(config::parse_selector)
        .unwrap_or(config::NodeSelector::Default);
    let (idx, node) = config::pick_node(&nodes, &selector)?;
    let picked = config::ProxyNode {
        name: node.name.clone(),
        protocol: node.protocol,
        address: node.address.clone(),
        port: node.port,
        outbound: node.outbound.clone(),
    };
    println!(
        "       使用节点: [{idx:02}] {} {} ({}:{})",
        picked.protocol, picked.name, picked.address, picked.port
    );

    let config_path = paths::xray_config_path()?;
    paths::ensure_parent(&config_path).await?;
    println!("[3/4] 生成本地配置: {}", config_path.display());
    let local = config::build_local_config(&picked.outbound, socks_port, http_port);
    let bytes = serde_json::to_vec_pretty(&local).context("序列化本地配置失败")?;
    tokio::fs::write(&config_path, &bytes)
        .await
        .with_context(|| format!("写入配置文件失败: {}", config_path.display()))?;

    println!("[4/4] 准备 xray 可执行文件...");
    let binary = xray::ensure_xray().await?;
    println!("       使用 xray: {}", binary.display());

    Ok((binary, config_path, idx, picked))
}

/// `on` / `start`：后台启动 xray。
pub async fn cmd_start(
    node_selector: Option<&str>,
    socks_port: u16,
    http_port: u16,
) -> Result<()> {
    if let Some(pid) = read_alive_pid()? {
        anyhow::bail!(
            "sunrise-xray 已在后台运行 (PID {}); 先 'sunrise-xray off' 再重启",
            pid
        );
    }

    #[cfg(not(unix))]
    {
        anyhow::bail!("Windows 暂不支持后台模式，请前台直接跑 sunrise-xray");
    }

    #[cfg(unix)]
    {
        let (binary, config_path, idx, node) =
            prepare(node_selector, socks_port, http_port).await?;
        spawn_xray_detached(&binary, &config_path, idx, &node, socks_port, http_port)
    }
}

#[cfg(unix)]
fn spawn_xray_detached(
    binary: &Path,
    config: &Path,
    idx: usize,
    node: &config::ProxyNode,
    socks_port: u16,
    http_port: u16,
) -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let log_path = paths::log_path()?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("打开日志文件失败: {}", log_path.display()))?;
    let log_err = log.try_clone()?;

    let mut cmd = Command::new(binary);
    cmd.arg("run")
        .arg("-c")
        .arg(config)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));

    // 让 xray 找到同目录的 geoip.dat / geosite.dat
    if let Some(dir) = binary.parent() {
        if dir.join("geoip.dat").is_file() {
            cmd.env("XRAY_LOCATION_ASSET", dir);
        }
    }

    // setsid 让 xray 脱离父进程的会话/进程组——
    // 这样父 sunrise-xray 退出 / 用户关 SSH 都不会带走 xray
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = cmd
        .spawn()
        .with_context(|| format!("启动 xray 失败: {}", binary.display()))?;
    let pid = child.id();

    // 等 500ms 看 xray 是否立刻挂掉（例如配置错）
    std::thread::sleep(Duration::from_millis(500));
    if !process_alive(pid) {
        anyhow::bail!(
            "xray 启动后立刻退出。查看日志: {}",
            log_path.display()
        );
    }

    std::fs::write(paths::pid_path()?, format!("{}\n", pid))
        .context("写 PID 文件失败")?;

    let state = DaemonState {
        pid,
        started_at: chrono::Utc::now(),
        node_name: node.name.clone(),
        node_index: idx,
        node_protocol: node.protocol.to_string(),
        socks_port,
        http_port,
    };
    write_state(&state)?;

    println!();
    println!("✓ sunrise-xray 已在后台运行 (PID {})", pid);
    println!("    节点  : [{}] {} ({})", idx, node.name, node.protocol);
    println!("    SOCKS5: socks5://127.0.0.1:{}", socks_port);
    println!("    HTTP  : http://127.0.0.1:{}", http_port);
    println!("    日志  : {}", log_path.display());
    println!();
    println!("操作：sunrise-xray status / test / off / logs -f");

    Ok(())
}

/// `off` / `stop`：停止后台 xray。
pub fn cmd_stop() -> Result<()> {
    let pid = match read_alive_pid()? {
        Some(p) => p,
        None => {
            println!("sunrise-xray 未在运行");
            clear_runtime_files()?;
            return Ok(());
        }
    };

    #[cfg(unix)]
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    // 给 xray 2 秒优雅退出
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(100));
        if !process_alive(pid) {
            clear_runtime_files()?;
            println!("✓ sunrise-xray 已停止 (PID {})", pid);
            return Ok(());
        }
    }

    // 不退就强杀
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
    std::thread::sleep(Duration::from_millis(200));
    clear_runtime_files()?;
    println!("⚠ sunrise-xray 已强制停止 (PID {})", pid);
    Ok(())
}

/// `restart`：stop + start。
pub async fn cmd_restart(
    node_selector: Option<&str>,
    socks_port: u16,
    http_port: u16,
) -> Result<()> {
    cmd_stop()?;
    std::thread::sleep(Duration::from_millis(300));
    cmd_start(node_selector, socks_port, http_port).await
}

/// `status`：看后台是否在跑 + 节点 + 端口 + 运行时长。
pub fn cmd_status() -> Result<()> {
    let pid_opt = read_alive_pid()?;
    let state_opt = read_state()?;

    match (pid_opt, state_opt) {
        (Some(pid), Some(st)) if pid == st.pid => {
            let uptime = chrono::Utc::now() - st.started_at;
            let uptime_str = format_duration(uptime);
            println!("● sunrise-xray 运行中");
            println!("    PID     : {}", pid);
            println!("    节点    : [{}] {} ({})", st.node_index, st.node_name, st.node_protocol);
            println!("    SOCKS5  : socks5://127.0.0.1:{}", st.socks_port);
            println!("    HTTP    : http://127.0.0.1:{}", st.http_port);
            println!("    已运行  : {}", uptime_str);
            println!("    启动于  : {}", st.started_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S"));
            println!("    日志    : {}", paths::log_path()?.display());
        }
        (Some(pid), _) => {
            // PID 文件存在但 state.json 缺失/不对齐（外部启动？）
            println!("● sunrise-xray 进程存活 (PID {}), 但找不到匹配的 state.json", pid);
            println!("    （可能是手动 kill 又 spawn 的，建议 'sunrise-xray off' 重置）");
        }
        (None, _) => {
            println!("○ sunrise-xray 未运行");
        }
    }
    Ok(())
}

fn format_duration(d: chrono::Duration) -> String {
    let secs = d.num_seconds().max(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    }
}

/// `test`：走代理 GET 几个目标，看哪些通哪些不通。
pub async fn cmd_test(http_port: u16) -> Result<()> {
    let proxy_url = format!("http://127.0.0.1:{}", http_port);

    // 预检：代理在不在监听
    if std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", http_port).parse().unwrap(),
        Duration::from_secs(2),
    )
    .is_err()
    {
        println!("✗ 本地代理端口 {} 没人监听。", http_port);
        println!("  先 'sunrise-xray on' 或前台跑 'sunrise-xray' 再测。");
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(&proxy_url).context("构造 proxy 失败")?)
        .timeout(Duration::from_secs(10))
        .build()?;

    // 目标顺序：先看 IP（确认走代理了），再看常用墙外站点
    let targets: &[(&str, &str, bool)] = &[
        ("出口 IP (ipify)",       "https://api.ipify.org",      true),
        ("出口归属 (ipinfo)",     "https://ipinfo.io/json",      true),
        ("Google",                "https://www.google.com",      false),
        ("GitHub",                "https://github.com",          false),
        ("Cloudflare",            "https://1.1.1.1",             false),
    ];

    println!("通过代理 {} 测试：", proxy_url);
    println!();

    let mut ok = 0;
    let mut total = 0;
    for (name, url, show_body) in targets {
        total += 1;
        let start = std::time::Instant::now();
        match client.get(*url).send().await {
            Ok(resp) => {
                let status = resp.status();
                let elapsed = start.elapsed();
                let ms = elapsed.as_millis();
                if *show_body {
                    let body = resp
                        .text()
                        .await
                        .unwrap_or_default()
                        .chars()
                        .take(200)
                        .collect::<String>();
                    println!("  ✓ {:<24} {} ({}ms)  {}", name, status, ms, body.trim().replace('\n', " "));
                } else {
                    println!("  ✓ {:<24} {} ({}ms)", name, status, ms);
                }
                if status.is_success() || status.is_redirection() {
                    ok += 1;
                }
            }
            Err(e) => {
                let msg = format!("{e}");
                let short = msg.chars().take(120).collect::<String>();
                println!("  ✗ {:<24} 失败: {}", name, short);
            }
        }
    }

    println!();
    println!("成功 {}/{}", ok, total);
    if ok == 0 {
        println!("提示：节点可能挂了或被墙了。换节点试试：sunrise-xray --node <其他> restart");
    }
    Ok(())
}
/// `use`：交互式选择节点。
///
/// 流程：拉订阅 → 并发测每个节点的 TCP 连接延迟（3s 超时）→ 按延迟排序 →
/// dialoguer Select 让用户上下选 → 选完后停掉旧 daemon、用新节点起 daemon。
pub async fn cmd_use(socks_port: u16, http_port: u16) -> Result<()> {
    use std::time::Instant;

    // 准备：订阅 → 节点列表
    let sub_url_raw = std::env::var("SUNRISE_SUB_URL")
        .map_err(|_| anyhow!("环境变量 SUNRISE_SUB_URL 未设置"))?;
    let sub_url = url::Url::parse(&sub_url_raw)
        .with_context(|| format!("SUNRISE_SUB_URL 不是合法 URL: {sub_url_raw}"))?;

    println!("[1/3] 拉取订阅...");
    let raw = fetch::fetch_subscription(sub_url.as_str()).await?;
    let nodes = config::parse_subscription(&raw);
    anyhow::ensure!(!nodes.is_empty(), "订阅里没有可用节点");

    println!("[2/3] 测试 {} 个节点延迟（3s 超时，并发）...", nodes.len());
    let latencies = measure_latencies(&nodes).await;

    // 用一个排序索引：先按延迟升序，超时的丢到最后；但显示时仍带原始 idx
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| match latencies[i] {
        Some(ms) => (0u8, ms),
        None => (1, u32::MAX),
    });

    let items: Vec<String> = order
        .iter()
        .map(|&i| {
            let lat_str = match latencies[i] {
                Some(ms) => format!("{:>5}ms", ms),
                None => "  超时 ".to_string(),
            };
            format!(
                "[{:02}] {}  {:<8}  {}",
                i, lat_str, nodes[i].protocol, nodes[i].name
            )
        })
        .collect();

    println!("[3/3] 选择节点：↑↓ 导航，Enter 确认，Esc/Ctrl+C 取消");
    let selection = dialoguer::Select::new()
        .with_prompt("当前可用节点")
        .items(&items)
        .default(0)
        .interact_opt()
        .context("交互式选择失败（非 TTY 环境下无法使用 'use'，请用 --node + restart）")?;

    let pick_in_order = match selection {
        Some(i) => i,
        None => {
            println!("已取消，未切换。");
            return Ok(());
        }
    };
    let orig_idx = order[pick_in_order];
    let chosen = &nodes[orig_idx];
    let lat = latencies[orig_idx];

    println!();
    println!(
        "→ 切换到 [{:02}] {} ({})  延迟: {}",
        orig_idx,
        chosen.name,
        chosen.protocol,
        lat.map(|ms| format!("{}ms", ms)).unwrap_or_else(|| "未知".into())
    );

    // 生成 xray 配置
    let config_path = paths::xray_config_path()?;
    paths::ensure_parent(&config_path).await?;
    let local = config::build_local_config(&chosen.outbound, socks_port, http_port);
    let bytes = serde_json::to_vec_pretty(&local).context("序列化本地配置失败")?;
    tokio::fs::write(&config_path, &bytes)
        .await
        .with_context(|| format!("写入配置文件失败: {}", config_path.display()))?;

    let binary = xray::ensure_xray().await?;

    // 停掉现有 daemon（如果在跑），等 300ms 给端口空出来
    cmd_stop()?;
    std::thread::sleep(Duration::from_millis(300));

    // 用 chosen 起新 daemon
    let owned_node = config::ProxyNode {
        name: chosen.name.clone(),
        protocol: chosen.protocol,
        address: chosen.address.clone(),
        port: chosen.port,
        outbound: chosen.outbound.clone(),
    };
    #[cfg(unix)]
    {
        spawn_xray_detached(&binary, &config_path, orig_idx, &owned_node, socks_port, http_port)?;
    }
    #[cfg(not(unix))]
    {
        let _ = (binary, config_path, owned_node);
        anyhow::bail!("Windows 暂不支持后台模式；前台跑请用 'sunrise-xray --node {} '", orig_idx);
    }

    // 顺便测一下新节点是否真的通了
    println!();
    println!("等 1 秒后测试新节点连通性...");
    let _ = Instant::now();
    tokio::time::sleep(Duration::from_secs(1)).await;
    cmd_test(http_port).await?;

    Ok(())
}

/// 并发测每个节点的 TCP connect 延迟。3 秒超时，超时记 None。
async fn measure_latencies(nodes: &[config::ProxyNode]) -> Vec<Option<u32>> {
    use tokio::net::TcpStream;
    use tokio::task::JoinSet;
    use tokio::time::{timeout, Instant};

    let mut set = JoinSet::new();
    for (i, n) in nodes.iter().enumerate() {
        let addr = format!("{}:{}", n.address, n.port);
        set.spawn(async move {
            let start = Instant::now();
            let r = timeout(Duration::from_secs(3), TcpStream::connect(&addr)).await;
            let lat = match r {
                Ok(Ok(_)) => Some(start.elapsed().as_millis() as u32),
                _ => None,
            };
            (i, lat)
        });
    }

    let mut results = vec![None; nodes.len()];
    while let Some(res) = set.join_next().await {
        if let Ok((i, lat)) = res {
            results[i] = lat;
        }
    }
    results
}

/// `logs`：看后台日志（-n N 显示最后 N 行，-f 持续跟踪）。
pub async fn cmd_logs(lines: usize, follow: bool) -> Result<()> {
    let log_path = paths::log_path()?;
    if !log_path.exists() {
        anyhow::bail!("日志文件不存在: {}", log_path.display());
    }

    // 先把最后 N 行打出来
    let content = std::fs::read_to_string(&log_path)?;
    let all: Vec<&str> = content.lines().collect();
    let start = all.len().saturating_sub(lines);
    for line in &all[start..] {
        println!("{}", line);
    }

    if !follow {
        return Ok(());
    }

    // -f：从当前文件尾部开始 poll，新内容来了就 print
    let mut last_len = std::fs::metadata(&log_path)?.len();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("\n停止跟踪");
                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {}
        }

        let now_len = match std::fs::metadata(&log_path) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        if now_len > last_len {
            // 读 last_len .. now_len 这段
            use std::io::{Read, Seek, SeekFrom};
            let mut f = std::fs::File::open(&log_path)?;
            f.seek(SeekFrom::Start(last_len))?;
            let mut buf = Vec::with_capacity((now_len - last_len) as usize);
            f.take(now_len - last_len).read_to_end(&mut buf)?;
            print!("{}", String::from_utf8_lossy(&buf));
            last_len = now_len;
        } else if now_len < last_len {
            // 日志被外部 rotate / truncate 了
            last_len = 0;
        }
    }
}
