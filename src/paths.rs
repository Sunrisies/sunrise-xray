use anyhow::{Context, Result};
use std::path::PathBuf;

const APP_NAME: &str = "sunrise-xray";

/// 用户级缓存根目录：
/// - macOS: `~/Library/Caches/sunrise-xray/`
/// - Linux: `$XDG_CACHE_HOME/sunrise-xray/` 或 `~/.cache/sunrise-xray/`
/// - Windows: `%LOCALAPPDATA%/sunrise-xray/`
pub fn cache_dir() -> Result<PathBuf> {
    let base = dirs::cache_dir()
        .context("无法定位用户缓存目录（dirs::cache_dir 返回 None）")?;
    Ok(base.join(APP_NAME))
}

/// xray 二进制 / 数据文件存放目录。
pub fn xray_bin_dir() -> Result<PathBuf> {
    Ok(cache_dir()?.join("bin"))
}

/// xray 可执行文件路径。Windows 自动带 .exe 后缀。
pub fn xray_bin_path() -> Result<PathBuf> {
    let name = if cfg!(windows) { "xray.exe" } else { "xray" };
    Ok(xray_bin_dir()?.join(name))
}

/// 每次运行重新生成的 xray 配置文件路径。
pub fn xray_config_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("xray_config.json"))
}

/// daemon 模式记录 xray 子进程 PID 的文件。
pub fn pid_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("sunrise-xray.pid"))
}

/// daemon 模式 xray 的 stdout/stderr 落地文件（status / logs 读这个）。
pub fn log_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("sunrise-xray.log"))
}

/// daemon 模式存的元信息：当时选了哪个节点、什么端口、什么时候启动的。
/// PID 在另外的 pid_path 里，是为了"PID 文件" Unix 习惯。
pub fn state_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("state.json"))
}

/// 确保父目录存在；不存在则创建。
pub async fn ensure_parent(path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("创建目录失败: {}", parent.display()))?;
    }
    Ok(())
}
