# sunrise-xray 开发者指南

> 面向贡献者和二次开发者。内容包括：环境搭建、项目结构速览、如何新增协议支持、测试体系、CI/CD 流程、发布清单。

---

## 目录

1. [环境搭建](#1-环境搭建)
2. [项目结构导览](#2-项目结构导览)
3. [从源码构建](#3-从源码构建)
4. [新增协议支持](#4-新增协议支持)
5. [测试体系](#5-测试体系)
6. [CI/CD 流程](#6-cicd-流程)
7. [发布清单](#7-发布清单)
8. [编码规范](#8-编码规范)

---

## 1. 环境搭建

### 系统要求

| 依赖 | 最低版本 | 说明 |
|------|----------|------|
| Rust | 1.85+ | Edition 2021，需 `rustup` 安装 |
| Cargo | 随 Rust | 标准包管理器 |
| 网络 | 首次编译需要 | 下载 xray-core release zip（~15MB） |

### 推荐工具

```bash
# Rust 工具链
rustup component add clippy rustfmt

# 代码质量
cargo install cargo-audit    # 安全审计
cargo install cargo-tarpaulin # 覆盖率

# 跨平台编译（可选）
cargo install cross
```

---

## 2. 项目结构导览

```
sunrise-xray/
├── build.rs                      # [核心] 编译期下载 xray-core + 嵌入
├── Cargo.toml                    # 依赖声明 + 元信息
├── Cross.toml                    # cross 容器配置（CI 跨平台编译用）
├── scripts/
│   └── install.sh                # 一键安装脚本（多镜像 fallback）
├── src/
│   ├── main.rs                   # CLI 入口 + 子命令分派
│   ├── commands.rs               # daemon 管理 + 交互选择 + 自愈
│   ├── fetch.rs                  # 订阅 HTTP 拉取 + 重试
│   ├── config.rs                 # 核心：URI 解析 + xray JSON 生成
│   ├── paths.rs                  # XDG 路径管理
│   ├── util.rs                   # 容错 base64 + URL 脱敏
│   ├── embedded.rs               # 嵌入 zip 释放 + 版本对账
│   └── xray.rs                   # xray 子进程管理
├── .github/workflows/
│   ├── ci.yml                    # PR/push 自动化
│   └── release.yml               # 发布自动化
└── docs/
    ├── architecture.md           # 架构设计文档
    ├── dev-guide.md              # 本文件
    └── deploy-guide.md           # 部署运维指南
```

### 职责矩阵

| 模块 | 职责 | 关键函数 |
|------|------|----------|
| `config.rs` | URI 解析 + JSON 配置生成 | `parse_subscription`, `build_local_config`, `build_stream_settings` |
| `fetch.rs` | HTTP 请求 + 重试逻辑 | `fetch_subscription` (3 次指数退避) |
| `commands.rs` | 业务流程编排 | `cmd_use`, `cmd_autoswitch`, `cmd_start`, `prepare` |
| `embedded.rs` | 嵌入资源释放 | `ensure_extracted` (版本对账 + 解压) |
| `xray.rs` | 进程生命周期 | `ensure_xray`, `run_xray` |
| `util.rs` | 共享工具函数 | `decode_base64_loose`, `redact_url` |
| `paths.rs` | 文件系统布局 | `cache_dir`, `xray_bin_path` |

---

## 3. 从源码构建

### 标准构建

```bash
git clone https://github.com/Sunrisies/sunrise-xray.git
cd sunrise-xray
cargo build --release
```

首次编译时 `build.rs` 会执行以下操作：
1. 自动探测 `CARGO_CFG_TARGET_OS` + `CARGO_CFG_TARGET_ARCH`
2. 查询 GitHub Release API 获取最新 xray-core 版本 + 对应平台 asset URL
3. 按镜像优先级下载 zip → SHA256 校验 → 嵌入二进制

> 国内用户注意：`build.rs` 内置了 ghproxy.net 等镜像，通常无需 VPN

### 离线构建

先在有网络的机器上下好对应平台的 Xray release zip：

```bash
# Linux x86_64 示例
wget https://github.com/XTLS/Xray-core/releases/latest/download/Xray-linux-64.zip
# 放到目标机器上
SUNRISE_XRAY_ZIP=/path/to/Xray-linux-64.zip cargo build --release
```

### 跨平台编译

```bash
# 安装 cross（基于 Docker 的跨平台编译）
cargo install cross

# 编译 ARM64 Linux 产物
cross build --release --target aarch64-unknown-linux-musl

# 编译 ARMv7 Linux 产物
cross build --release --target armv7-unknown-linux-musleabihf
```

`build.rs` 会自动根据 `CARGO_CFG_TARGET_*` 选择正确平台 asset：

| 编译目标 | 对应 xray asset |
|----------|----------------|
| `x86_64-apple-darwin` | `Xray-macos-64.zip` |
| `aarch64-apple-darwin` | `Xray-macos-arm64-v8a.zip` |
| `x86_64-unknown-linux-musl` | `Xray-linux-64.zip` |
| `aarch64-unknown-linux-musl` | `Xray-linux-arm64-v8a.zip` |
| `armv7-unknown-linux-musleabihf` | `Xray-linux-arm32-v7a.zip` |
| `i686-unknown-linux-musl` | `Xray-linux-32.zip` |
| `x86_64-pc-windows-msvc` | `Xray-windows-64.zip` |
| `aarch64-pc-windows-msvc` | `Xray-windows-arm64-v8a.zip` |
| `i686-pc-windows-msvc` | `Xray-windows-32.zip` |

### 调试构建

```bash
# debug 模式 + 日志
cargo build
RUST_BACKTRACE=1 SUNRISE_SUB_URL='https://...' ./target/debug/sunrise-xray list
```

---

## 4. 新增协议支持

> sunrise-xray 目前支持 VLESS / VMess / Trojan / Shadowsocks。如果你需要添加新的协议（如 Hysteria2、SOCKS5 出站等），以下是详细步骤。

### 4.1 理解现有的解析框架

订阅解析流程：

```
fetch.rs: HTTP GET → base64 decode（util.rs）
                ↓
config.rs: parse_subscription()
                ↓
    逐行识别 scheme → dispatch 到对应 parser
                ↓
    parser 返回 ProxyNode { name, protocol, address, port, outbound: Value }
                ↓
build_local_config() 把 outbound 塞进完整 xray 配置
```

### 4.2 四步添加新协议

**Step 1**: 在 `src/config.rs` 的 `parse_subscription` 中添加 scheme 识别

```rust
// 在 parse_subscription 的 match 块中添加新分支
let parsed = match scheme.as_str() {
    "vless" => parse_vless_uri(line),
    "trojan" => parse_trojan_uri(line),
    "ss" => parse_ss_uri(line),
    "vmess" => parse_vmess_uri(line),
    "hysteria2" => parse_hysteria2_uri(line),  // ← 新增
    _ => { /* 跳过 */ continue },
};
```

**Step 2**: 实现 URI 解析函数

```rust
fn parse_hysteria2_uri(line: &str) -> Result<ProxyNode> {
    let url = Url::parse(line).context("hysteria2 URL 解析失败")?;

    // 1. 提取认证信息（userinfo / password 等）
    let password = url.username().to_string();

    // 2. 提取服务器地址和端口
    let address = url.host_str().context("缺少 host")?.to_string();
    let port = url.port().context("缺少 port")?;

    // 3. 提取查询参数（obfs、sni、insecure 等）
    let q: HashMap<String, String> = url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    // 4. 获取节点名称（#fragment 或 fallback）
    let name = fragment_name(&url);

    // 5. 构造 xray outbound JSON（注：Hysteria2 需要 sing-box 核心，这里仅为示例）
    let outbound = json!({
        "tag": "proxy",
        "protocol": "hysteria2",
        "settings": { /* 按 xray/sing-box 配置格式 */ },
        "streamSettings": { /* 传输层配置 */ },
    });

    Ok(ProxyNode { name, protocol: "hysteria2", address, port, outbound })
}
```

**Step 3**: 在 `build_local_config` 中不需要改——xray 配置格式由 `outbound` JSON 自行携带。

**Step 4**: 添加单元测试

```rust
#[test]
fn parse_hysteria2_basic() {
    let text = "hysteria2://password@example.com:443?insecure=1&sni=cdn.example.com#HY2";
    let nodes = parse_subscription(text);
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].protocol, "hysteria2");
    // ... 更多字段断言
}
```

### 4.3 添加新传输层

在 `build_stream_settings` 中添加新 network 类型分支：

```rust
match network {
    "tcp" => {}
    "ws" => { stream["wsSettings"] = build_ws_settings(q); }
    "grpc" => { stream["grpcSettings"] = build_grpc_settings(q); }
    "http" => { stream["httpSettings"] = build_http_settings(q); }
    "kcp" => { stream["kcpSettings"] = build_kcp_settings(q); }  // ← 新增
    other => anyhow::bail!("暂不支持的传输层: type={other}"),
}
```

### 4.4 关于协议兼容的注意事项

- xray-core 原生不支持的协议（如 Hysteria2）需要额外的核心集成，超出了本项目的 scope
- 新增协议时请确保 URI 格式有公开规范参考（如 SIP002 for SS、v2rayN JSON for VMess）
- 遇到无法解析的节点应该在 stderr 打印提示而非静默丢弃

---

## 5. 测试体系

### 5.1 单元测试

目前有 **42+ 个单元测试**，集中在配置解析核心逻辑：

```bash
# 运行所有测试
cargo test

# 只跑 config 模块测试
cargo test -- config

# 运行测试并显示输出
cargo test -- --nocapture

# 特定测试（按名称过滤）
cargo test parse_subscription_vless_reality
```

测试覆盖内容：

| 模块 | 测试重点 | 数量 |
|------|----------|------|
| `config.rs` | 各协议 URI 解析、传输层构建、节点选择、配置生成 | 30+ |
| `util.rs` | 容错 base64 解码、URL 脱敏 | 8 |
| `embedded.rs` | zip 有效性、解压完整性、版本标记 | 4 |

### 5.2 端到端测试（TODO）

项目尚未覆盖端到端测试，以下是建议方案：

```rust
// tests/e2e.rs（建议位置）
// 需要一个真实或 mock 的订阅服务器 + 可预测的测试数据

#[tokio::test]
async fn test_fetch_and_parse_roundtrip() {
    // 1. 启动 mock HTTP server 返回预设订阅内容
    // 2. 调用 fetch_subscription
    // 3. 调用 parse_subscription
    // 4. 验证节点列表
}
```

### 5.3 手动测试清单

```bash
# 1. 基本功能
cargo build --release
SUNRISE_SUB_URL='https://你的订阅' ./target/release/sunrise-xray list
SUNRISE_SUB_URL='https://你的订阅' ./target/release/sunrise-xray --node 0

# 2. 交互模式
SUNRISE_SUB_URL='https://你的订阅' ./target/release/sunrise-xray use

# 3. daemon 生命周期
SUNRISE_SUB_URL='https://你的订阅' ./target/release/sunrise-xray on
./target/release/sunrise-xray status
./target/release/sunrise-xray test
./target/release/sunrise-xray off

# 4. 后台日志
./target/release/sunrise-xray logs -n 20
```

---

## 6. CI/CD 流程

### CI Pipeline (`.github/workflows/ci.yml`)

| 触发条件 | 执行内容 |
|----------|----------|
| push 到 `main` | `cargo check` + `cargo test` |
| 任何 PR | 同上 + Clippy lint |

分支保护建议：main 分支要求 CI 通过才能合入。

### Release Pipeline (`.github/workflows/release.yml`)

| 触发条件 | 执行内容 |
|----------|----------|
| 推 `v*` tag（如 `v0.4.0`） | 5 平台交叉编译 → GitHub Release → Qiniu CDN 镜像 |
| workflow_dispatch（手动） | 仅编译做 dry-run，不创建 Release |

release 产物：

| 文件 | 对应平台 |
|------|----------|
| `sunrise-xray-*-x86_64-apple-darwin.tar.gz` | Intel Mac |
| `sunrise-xray-*-aarch64-apple-darwin.tar.gz` | Apple Silicon |
| `sunrise-xray-*-x86_64-unknown-linux-musl.tar.gz` | x86_64 Linux |
| `sunrise-xray-*-aarch64-unknown-linux-musl.tar.gz` | ARM64 Linux |
| `sunrise-xray-*-x86_64-pc-windows-msvc.zip` | Windows x86_64 |
| 每个产物带同名 `.sha256` 校验文件 | — |

Release 后自动更新：
- Qiniu CDN 镜像（`https://cdn.sunrise1024.top/sunrise-xray/<tag>/`）
- `latest.txt` 版本指针
- 触发 CDN 缓存刷新（qshell cdnrefresh）

### 预发布版本

tag 包含 `-` 的（如 `v0.4.0-rc1`）自动标记为 GitHub prerelease。

---

## 7. 发布清单

发布新版本时按以下步骤操作：

```bash
# 1. 更新 Cargo.toml 版本号
#    version = "0.4.0"

# 2. 更新 docs/ 中的版本引用（如有）
#    搜索之前的版本号替换

# 3. 提交 + tag
git commit -am "Release v0.4.0"
git tag v0.4.0

# 4. 推送（触发 CI/CD）
git push origin main v0.4.0

# 5. [可选] 在 GitHub Release 页面补充 release notes
```

### 发布前检查清单

- [ ] `cargo test` 全部通过
- [ ] `cargo clippy` 无 warning
- [ ] `cargo build --release` 能正常编译
- [ ] `scripts/install.sh` 能正确安装新版本产物
- [ ] 6 个平台 CI artifact 全部绿色（macOS x2 + Linux x2 + Windows x2）
- [ ] CHANGELOG（或 release notes）已更新
- [ ] Qiniu CDN `latest.txt` 已更新（由 CI 自动做，但需确认）

---

## 8. 编码规范

### Rust 风格

- 遵循标准 Rust 命名规范：`snake_case` 函数/变量、`PascalCase` 类型、`SCREAMING_CASE` 常量
- 所有公开函数和类型加 doc comment
- 使用 `anyhow::Result` 做错误传递，错误消息用中文（目标用户群体为中文用户）
- 关键业务逻辑写 `///` 注释说明 WHY 而非 WHAT

### 模块导入惯例

```rust
// 标准库优先
use std::path::{Path, PathBuf};
use std::time::Duration;

// 第三方 crate
use anyhow::{Context, Result};
use serde_json::{json, Value};

// 本 crate 模块
use crate::paths;
```

### 错误处理

```rust
// ❌ 不要：丢失上下文的 bare unwrap
let x = foo().unwrap();

// ✅ 应该：带上下文描述
let x = foo().with_context(|| format!("处理失败: {}", path.display()))?;

// ❌ 不要：在库函数里 println / eprintln
// ✅ 应该：返回 Err，让上层决定怎么展示
```

### 测试

测试函数名描述清楚测试场景：

```rust
#[test]
fn parse_subscription_vless_reality() { /* ... */ }
#[test]
fn parse_subscription_skips_unsupported_scheme() { /* ... */ }
```

---

## 附录：常见构建问题

| 问题 | 原因 | 解决方案 |
|------|------|----------|
| `build.rs` 下载失败 | 网络不通 | `SUNRISE_XRAY_ZIP=/path/to/Xray-xxx.zip cargo build` |
| `cross` 编译卡住 | Docker 首次拉镜像 | 先 `cross build --target aarch64-unknown-linux-musl --check` |
| `cargo test` 中 `embedded::tests` 失败 | 测试写入 `/tmp` 权限 | `rm -rf /tmp/sunrise-xray-test-* && cargo test` |
| 运行时 `libc` 找不到 | 非 Unix 平台 | `cfg(unix)` 的条件编译会自动排除 Windows |
