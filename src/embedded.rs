use crate::paths;
use anyhow::{Context, Result};
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// 编译时下载好的 xray release zip。`include_bytes!` 在 binary 里固化这些字节。
pub const XRAY_ZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/xray.zip"));

/// 编译时 xray 的 release tag（例如 `v26.3.27`），或 `vendored` 表示离线 zip。
/// 用于在 cache 里做版本对账：升级 binary → 重新解压。
pub const XRAY_VERSION: &str = include_str!(concat!(env!("OUT_DIR"), "/xray_version.txt"));

/// 把嵌入的 zip 释放到 cache 目录（`~/Library/Caches/sunrise-xray/bin/` 等），
/// 返回 xray 可执行文件路径。
///
/// 通过 `version.tag` 文件做"已经释放过"的对账，避免每次启动都重写一遍 ~30MB。
pub async fn ensure_extracted() -> Result<PathBuf> {
    let bin_dir = paths::xray_bin_dir()?;
    let target = paths::xray_bin_path()?;
    let marker = bin_dir.join("version.tag");

    let need_extract = match tokio::fs::read_to_string(&marker).await {
        Ok(s) => s.trim() != XRAY_VERSION.trim() || !target.is_file(),
        Err(_) => true,
    };

    if !need_extract {
        return Ok(target);
    }

    tokio::fs::create_dir_all(&bin_dir)
        .await
        .with_context(|| format!("创建目录失败: {}", bin_dir.display()))?;

    let dir_for_task = bin_dir.clone();
    tokio::task::spawn_blocking(move || extract_zip(&dir_for_task))
        .await
        .context("解压任务 join 失败")??;

    tokio::fs::write(&marker, XRAY_VERSION.trim())
        .await
        .with_context(|| format!("写 version.tag 失败: {}", marker.display()))?;

    Ok(target)
}

fn extract_zip(target_dir: &Path) -> Result<()> {
    let reader = Cursor::new(XRAY_ZIP);
    let mut archive = zip::ZipArchive::new(reader).context("打开嵌入的 zip 失败")?;

    let binary_name = if cfg!(windows) { "xray.exe" } else { "xray" };
    let mut got_binary = false;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        match name.as_str() {
            "xray" | "xray.exe" => {
                let dst = target_dir.join(binary_name);
                let mut out = std::fs::File::create(&dst)
                    .with_context(|| format!("创建文件失败: {}", dst.display()))?;
                std::io::copy(&mut entry, &mut out)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))?;
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

    anyhow::ensure!(got_binary, "嵌入的 zip 里找不到 xray 二进制");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_zip_is_a_valid_archive() {
        let reader = Cursor::new(XRAY_ZIP);
        let archive = zip::ZipArchive::new(reader).expect("embedded zip should be parseable");
        assert!(archive.len() > 0, "embedded zip should not be empty");
    }

    #[test]
    fn embedded_zip_contains_xray_binary() {
        let reader = Cursor::new(XRAY_ZIP);
        let mut archive = zip::ZipArchive::new(reader).unwrap();
        let mut has_binary = false;
        for i in 0..archive.len() {
            let entry = archive.by_index(i).unwrap();
            if entry.name() == "xray" || entry.name() == "xray.exe" {
                has_binary = true;
                break;
            }
        }
        assert!(has_binary, "embedded zip should contain xray or xray.exe");
    }

    #[test]
    fn embedded_version_is_not_empty() {
        assert!(
            !XRAY_VERSION.trim().is_empty(),
            "XRAY_VERSION should be populated by build.rs"
        );
    }

    #[test]
    fn extract_zip_writes_runnable_binary_to_tempdir() {
        let dir = std::env::temp_dir().join(format!(
            "sunrise-xray-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        extract_zip(&dir).expect("extract should succeed");

        let bin = dir.join(if cfg!(windows) { "xray.exe" } else { "xray" });
        let meta = std::fs::metadata(&bin).expect("xray binary should exist");
        assert!(meta.len() > 1_000_000, "xray binary should be > 1MB");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(meta.permissions().mode() & 0o111, 0o111, "xray should be executable");
        }
        assert!(dir.join("geoip.dat").is_file(), "geoip.dat should be extracted");
        assert!(dir.join("geosite.dat").is_file(), "geosite.dat should be extracted");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

