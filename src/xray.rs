use crate::embedded;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tokio::signal;

/// 解析 xray 可执行文件位置。
///
/// 1. 如果设置了 `XRAY_PATH` 且文件存在，使用它（高级用户用别的 xray 版本时）
/// 2. 否则把编译期嵌入的 xray 二进制释放到 cache 目录后使用
pub async fn ensure_xray() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("XRAY_PATH") {
        let pb = PathBuf::from(&p);
        if pb.is_file() {
            return Ok(pb);
        }
        eprintln!("XRAY_PATH 指向的文件不存在: {p}，回退到内置 xray");
    }
    embedded::ensure_extracted().await
}

/// 启动 xray，阻塞直到 Ctrl+C 或子进程退出。
/// kill_on_drop 保证主程序异常退出时子进程也会被回收。
pub async fn run_xray(binary: &Path, config: &Path) -> Result<()> {
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
