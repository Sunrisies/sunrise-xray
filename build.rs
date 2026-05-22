use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

const RELEASES_API: &str = "https://api.github.com/repos/XTLS/Xray-core/releases/latest";

/// 编译期下载的镜像列表。把直连 GitHub 放最后，国内网络不卡 90 秒。
const MIRRORS: &[&str] = &[
    "https://ghproxy.net/",
    "https://gh-proxy.com/",
    "https://ghps.cc/",
    "https://hub.gitmirror.com/",
    "",
];

fn main() {
    if let Err(e) = run() {
        // 打印到 stderr，cargo build 会把它当作 build script 失败原因
        eprintln!("\n[sunrise-xray build.rs 失败] {e:#}\n");
        eprintln!("可选规避方式：");
        eprintln!("  1) 用 VPN / 镜像让编译机能访问 GitHub release");
        eprintln!("  2) 手动下载对应平台的 Xray-core release zip，");
        eprintln!("     设置 SUNRISE_XRAY_ZIP=/path/to/Xray-xxx.zip 再 cargo build");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    // 仅在以下变化时重跑：build.rs 自身改了 / 离线 zip 路径改了 / 目标平台改了 / token 改了
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SUNRISE_XRAY_ZIP");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    println!("cargo:rerun-if-env-changed=GITHUB_TOKEN");
    println!("cargo:rerun-if-env-changed=GH_TOKEN");

    let out_dir: PathBuf = env::var("OUT_DIR").context("OUT_DIR 未设置")?.into();
    let zip_dest = out_dir.join("xray.zip");
    let version_dest = out_dir.join("xray_version.txt");

    // 已经下载过且文件齐全：跳过
    if zip_dest.is_file() && version_dest.is_file() && fs::metadata(&zip_dest)?.len() > 1024 {
        return Ok(());
    }

    // 离线逃生口：用户预先下好对应平台的 zip
    if let Ok(p) = env::var("SUNRISE_XRAY_ZIP") {
        println!("cargo:warning=使用预下载的 zip: {p}");
        let bytes = fs::read(&p).with_context(|| format!("读取预下载 zip 失败: {p}"))?;
        anyhow::ensure!(bytes.len() > 1024, "预下载 zip 体积异常 ({} bytes)", bytes.len());
        fs::write(&zip_dest, &bytes)?;
        fs::write(&version_dest, "vendored")?;
        return Ok(());
    }

    let asset_name = pick_asset_name()?;
    println!("cargo:warning=下载 xray release: {asset_name}（首次编译需要 30 秒~几分钟）");

    let agent = build_agent();
    let (zip_bytes, tag) = fetch_release_zip(&agent, asset_name)?;

    fs::write(&zip_dest, &zip_bytes).context("写 OUT_DIR/xray.zip 失败")?;
    fs::write(&version_dest, &tag).context("写 OUT_DIR/xray_version.txt 失败")?;
    println!("cargo:warning=已嵌入 {asset_name} ({tag}), 体积 {} bytes", zip_bytes.len());
    Ok(())
}

fn pick_asset_name() -> Result<&'static str> {
    let os = env::var("CARGO_CFG_TARGET_OS").context("CARGO_CFG_TARGET_OS 未设置")?;
    let arch = env::var("CARGO_CFG_TARGET_ARCH").context("CARGO_CFG_TARGET_ARCH 未设置")?;
    Ok(match (os.as_str(), arch.as_str()) {
        ("macos", "aarch64") => "Xray-macos-arm64-v8a.zip",
        ("macos", "x86_64") => "Xray-macos-64.zip",
        ("linux", "x86_64") => "Xray-linux-64.zip",
        ("linux", "aarch64") => "Xray-linux-arm64-v8a.zip",
        ("linux", "arm") => "Xray-linux-arm32-v7a.zip",
        ("linux", "x86") => "Xray-linux-32.zip",
        ("windows", "x86_64") => "Xray-windows-64.zip",
        ("windows", "aarch64") => "Xray-windows-arm64-v8a.zip",
        ("windows", "x86") => "Xray-windows-32.zip",
        (o, a) => anyhow::bail!("不支持的目标平台: {o}/{a}"),
    })
}

fn build_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent(concat!("sunrise-xray-build/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()
}

fn fetch_release_zip(agent: &ureq::Agent, asset_name: &str) -> Result<(Vec<u8>, String)> {
    let release: serde_json::Value = get_json(agent, RELEASES_API)
        .context("查询 GitHub release API 失败")?;

    let tag = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .context("release JSON 缺少 tag_name")?
        .to_string();

    let asset = release
        .get("assets")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(asset_name))
        })
        .with_context(|| format!("release 中未找到资产: {asset_name}"))?;

    let zip_url = asset
        .get("browser_download_url")
        .and_then(|v| v.as_str())
        .context("资产缺少 browser_download_url")?
        .to_string();

    let expected_sha256 = asset
        .get("digest")
        .and_then(|v| v.as_str())
        .and_then(|s| s.strip_prefix("sha256:"))
        .context("资产缺少 digest 字段，无法校验完整性")?
        .to_string();

    let bytes = download_with_mirrors(agent, &zip_url, |b| {
        anyhow::ensure!(b.len() >= 1024, "响应体过小 ({} bytes)", b.len());
        Ok(())
    })?;

    let actual = sha256_hex(&bytes);
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(&expected_sha256),
        "SHA256 不匹配\n  期望: {expected_sha256}\n  实际: {actual}"
    );

    Ok((bytes, tag))
}

fn get_json(agent: &ureq::Agent, url: &str) -> Result<serde_json::Value> {
    let mut req = agent.get(url);
    if let Some(token) = github_token() {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let body = req
        .call()
        .with_context(|| format!("GET {url} 失败"))?
        .into_string()
        .context("读取 JSON 响应失败")?;
    serde_json::from_str(&body).context("解析 JSON 失败")
}

/// 读取 GitHub 认证 token：优先 GITHUB_TOKEN（GitHub Actions 默认注入），
/// 其次 GH_TOKEN（gh CLI 习惯名）。带 token 时 api.github.com 速率限制
/// 从匿名 60 次/小时 提升到 5000 次/小时，CI 上多并行 job 才不会被 403。
fn github_token() -> Option<String> {
    for name in &["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(v) = env::var(name) {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

fn download_with_mirrors<F>(
    agent: &ureq::Agent,
    github_url: &str,
    validate: F,
) -> Result<Vec<u8>>
where
    F: Fn(&[u8]) -> Result<()>,
{
    let mut last_err: Option<anyhow::Error> = None;
    for prefix in MIRRORS {
        let url = if prefix.is_empty() {
            github_url.to_string()
        } else {
            format!("{prefix}{github_url}")
        };
        let label = if prefix.is_empty() { "直连 GitHub" } else { prefix };
        println!("cargo:warning=  下载({label}): {url}");

        match try_get_bytes(agent, &url) {
            Ok(b) => match validate(&b) {
                Ok(()) => return Ok(b),
                Err(e) => {
                    println!("cargo:warning=    响应校验失败: {e:#}");
                    last_err = Some(e);
                }
            },
            Err(e) => {
                println!("cargo:warning=    失败: {e:#}");
                last_err = Some(e);
            }
        }
    }
    Err(last_err
        .unwrap_or_else(|| anyhow::anyhow!("没有可用的下载源"))
        .context("所有镜像均下载失败"))
}

fn try_get_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>> {
    let resp = agent
        .get(url)
        .timeout(Duration::from_secs(60))
        .call()
        .context("call 失败")?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(50 * 1024 * 1024) // 50MB 上限，xray release 远小于此
        .read_to_end(&mut bytes)
        .context("读取响应体失败")?;
    Ok(bytes)
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(64);
    use std::fmt::Write;
    for b in digest {
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}
