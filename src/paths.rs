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

/// 确保父目录存在；不存在则创建。
pub async fn ensure_parent(path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("创建目录失败: {}", parent.display()))?;
    }
    Ok(())
}
