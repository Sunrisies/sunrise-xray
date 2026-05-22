use anyhow::{Context, Result};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::signal;

const RELEASES_API: &str = "https://api.github.com/repos/XTLS/Xray-core/releases/latest";
const CACHE_DIR: &str = "/tmp/sunrise-xray-bin";

/// 下载 zip 时尝试的 URL 前缀，空串代表直连 GitHub。
/// 这些镜像把 `https://github.com/...` 当作路径转发，所以 `format!("{prefix}{github_url}")` 即可。
const GH_MIRRORS: &[&str] = &[
    "",
    "https://ghproxy.net/",
    "https://gh-proxy.com/",
    "https://ghps.cc/",
    "https://hub.gitmirror.com/",
];

/// 解析当前 macOS 架构对应的 release 资产名
fn macos_asset_name() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "aarch64" => Ok("Xray-macos-arm64-v8a.zip"),
        "x86_64" => Ok("Xray-macos-64.zip"),
        other => anyhow::bail!("不支持的 macOS 架构: {other}"),
    }
}

/// 优先使用 XRAY_PATH 环境变量 / which xray / 本地缓存；都没有则从 GitHub 下载。
pub async fn ensure_xray() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("XRAY_PATH") {
        let pb = PathBuf::from(&p);
        if pb.is_file() {
            return Ok(pb);
        } else {
            eprintln!("XRAY_PATH 指向的文件不存在: {p}，回退到自动查找");
        }
    }

    if let Some(found) = which_xray().await {
        return Ok(found);
    }

    let cached = PathBuf::from(CACHE_DIR).join("xray");
    if cached.is_file() {
        return Ok(cached);
    }

    println!("未找到系统 xray，开始从 GitHub 下载最新版本...");
    download_latest_xray(&cached).await?;
    Ok(cached)
}

async fn which_xray() -> Option<PathBuf> {
    let out = Command::new("which").arg("xray").output().await.ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

async fn download_latest_xray(target: &Path) -> Result<()> {
    let asset_name = macos_asset_name()?;
    let client = reqwest::Client::builder()
        .user_agent("sunrise-xray/0.1")
        // 整体超时给宽一点，单次请求超时在 try_get_bytes 里另设
        .timeout(Duration::from_secs(300))
        .build()?;

    let release: serde_json::Value = client
        .get(RELEASES_API)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .context("查询 GitHub release 失败")?
        .error_for_status()
        .context("GitHub release API 返回非 2xx（可能被限流）")?
        .json()
        .await
        .context("解析 GitHub release JSON 失败")?;

    let download_url = release
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|arr| {
            arr.iter().find(|a| {
                a.get("name").and_then(|n| n.as_str()) == Some(asset_name)
            })
        })
        .and_then(|a| a.get("browser_download_url").and_then(|u| u.as_str()))
        .with_context(|| format!("release 中未找到资产: {asset_name}"))?
        .to_string();

    let bytes = download_with_mirrors(&client, &download_url).await?;

    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let target_buf = target.to_path_buf();
    let bytes_vec = bytes.to_vec();
    tokio::task::spawn_blocking(move || extract_xray_bin(&bytes_vec, &target_buf))
        .await
        .context("解压任务 join 失败")??;

    println!("xray 已安装到: {}", target.display());
    Ok(())
}

/// 按 GH_MIRRORS 顺序尝试下载；任何一个成功就返回。
async fn download_with_mirrors(client: &reqwest::Client, github_url: &str) -> Result<Vec<u8>> {
    let mut last_err: Option<anyhow::Error> = None;

    for prefix in GH_MIRRORS {
        let url = if prefix.is_empty() {
            github_url.to_string()
        } else {
            format!("{prefix}{github_url}")
        };
        let label = if prefix.is_empty() { "直连 GitHub" } else { prefix };
        println!("下载({label}): {url}");

        match try_get_bytes(client, &url).await {
            Ok(b) => return Ok(b),
            Err(e) => {
                println!("       失败: {e:#}");
                last_err = Some(e);
            }
        }
    }

    Err(last_err
        .unwrap_or_else(|| anyhow::anyhow!("没有可用的下载源"))
        .context("所有镜像均下载失败"))
}

async fn try_get_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(90))
        .send()
        .await
        .context("发起请求失败")?
        .error_for_status()
        .context("非 2xx 响应")?;
    let bytes = resp.bytes().await.context("读取响应体失败")?;
    // 镜像偶尔会返回一个 HTML 错误页或者很小的占位，做一下基本 sanity check
    if bytes.len() < 1024 {
        anyhow::bail!("响应体过小 ({} bytes)，可能不是有效 zip", bytes.len());
    }
    Ok(bytes.to_vec())
}

fn extract_xray_bin(zip_bytes: &[u8], target: &Path) -> Result<()> {
    let reader = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader).context("打开 zip 失败")?;

    let target_dir = target.parent().context("无效目标路径")?;
    let mut got_binary = false;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        match name.as_str() {
            "xray" => {
                let mut out = std::fs::File::create(target)
                    .with_context(|| format!("创建文件失败: {}", target.display()))?;
                std::io::copy(&mut entry, &mut out)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))?;
                }
                got_binary = true;
            }
            "geoip.dat" | "geosite.dat" => {
                let dst = target_dir.join(&name);
                let mut out = std::fs::File::create(&dst)
                    .with_context(|| format!("创建文件失败: {}", dst.display()))?;
                std::io::copy(&mut entry, &mut out)?;
            }
            _ => {}
        }
    }

    if !got_binary {
        anyhow::bail!("zip 内未找到 xray 二进制");
    }
    Ok(())
}

/// 启动 xray，阻塞直到 Ctrl+C 或子进程退出。
/// kill_on_drop 保证主程序异常退出时子进程也会被回收。
pub async fn run_xray(binary: &Path, config: &str) -> Result<()> {
    let mut cmd = Command::new(binary);
    cmd.arg("run")
        .arg("-c")
        .arg(config)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);

    // 如果 binary 同目录里有 geoip.dat / geosite.dat，主动设 XRAY_LOCATION_ASSET，
    // 避免 xray 因为找不到数据文件而启动失败。这对 brew 安装的 xray 是无感的
    // （它自己会去 /opt/homebrew/share/xray/ 找），对自下载的尤其重要。
    if let Some(dir) = binary.parent() {
        if dir.join("geoip.dat").is_file() {
            cmd.env("XRAY_LOCATION_ASSET", dir);
        }
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("启动 xray 失败: {}", binary.display()))?;

    tokio::select! {
        status = child.wait() => {
            let status = status.context("等待 xray 进程失败")?;
            if !status.success() {
                anyhow::bail!("xray 异常退出: {status}");
            }
            Ok(())
        }
        _ = signal::ctrl_c() => {
            println!("\n收到 Ctrl+C，正在停止 xray...");
            let _ = child.kill().await;
            let _ = child.wait().await;
            Ok(())
        }
    }
}
