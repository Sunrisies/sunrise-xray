<div align="center">

# ☀️ sunrise-xray

**订阅即代理 — 把机场订阅变成你电脑上的本地代理，零配置、单文件、开箱即跑**

[![CI](https://github.com/Sunrisies/sunrise-xray/actions/workflows/ci.yml/badge.svg)](https://github.com/Sunrisies/sunrise-xray/actions/workflows/ci.yml)
[![Release](https://github.com/Sunrisies/sunrise-xray/actions/workflows/release.yml/badge.svg)](https://github.com/Sunrisies/sunrise-xray/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85+-blue?logo=rust)](https://www.rust-lang.org)
[![Xray](https://img.shields.io/badge/Xray-Core-brightgreen?logo=cloudflare)](https://github.com/XTLS/Xray-core)
[![Static Badge](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux%20%7C%20Windows-blue)]()

**English** · [中文](README.md) · [架构文档](docs/architecture.md) · [开发者指南](docs/dev-guide.md) · [部署运维](docs/deploy-guide.md)

---

</div>

> 🚀 **一句话**：把机场订阅链接一扔，`sunrise-xray use` 选节点、测延迟、后台跑代理，一条命令搞定。curl / git / pip / brew 全自动走代理，终端开箱即用。

---

## ✨ 核心亮点

| 特性 | 说明 |
|------|------|
| **🔌 单文件交付** | 编译期嵌入 xray-core 二进制 + geoip/geosite 数据文件，产物就是一个可执行文件，丢哪都能跑 |
| **🌐 全协议支持** | VLESS (REALITY/TLS/none) · VMess · Trojan · Shadowsocks<br>TCP / WebSocket / gRPC / HTTP/2 传输层全覆盖 |
| **⚡ 交互式节点选择** | `sunrise-xray use` — 并发测全部节点延迟、按快慢排序、↑↓ 选、Enter 即切 |
| **🩺 自愈守护** | `autoswitch` 子命令定期健康检查，节点挂了自动切到最优活节点，cron 友好 |
| **🔒 隐私优先** | 所有错误日志中的订阅 URL 自动脱敏成 `scheme://host/***`，分享调试不泄露 token |
| **📦 极速安装** | 一键脚本自动探测平台、Qiniu CDN → ghproxy → GitHub 多级镜像、SHA256 校验 |
| **🖥️ Daemon 管理** | 内置 `on/off/restart/status/logs`，无需 systemd / launchd 即可后台常驻 |
| **🔁 指数退避重试** | 订阅请求网络抖动自动重试（最多 3 次），4xx 错误立即失败不浪费时间 |

---

## ⚡ 30 秒上手

```bash
# 1. 安装（macOS + Linux 通用）
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash

# 2. 设置订阅地址
export SUNRISE_SUB_URL='https://你的订阅地址'

# 3. 交互选节点 + 后台启动 + 自动测试
sunrise-xray use
```

完成后 `curl https://www.google.com` 就已经走代理了。详细使用见 [部署运维指南](docs/deploy-guide.md)。

---

## 🔧 子命令速查

```bash
sunrise-xray use             # ★ 推荐：交互选节点（测延迟 → ↑↓ 选 → 自动切换）
sunrise-xray autoswitch      # 健康检查 + 失活自动切最优节点（cron 用）
sunrise-xray on              # 后台启动（同义词：start）
sunrise-xray off             # 停止后台（同义词：stop）
sunrise-xray restart         # stop + start
sunrise-xray status          # 看 PID / 节点 / 端口 / 运行时长
sunrise-xray test            # 走代理 GET Google/GitHub/ipify
sunrise-xray logs            # 看后台日志（默认最后 50 行）
sunrise-xray logs -f         # 持续跟踪（Ctrl+C 停）
sunrise-xray logs -n 200     # 看最后 200 行
sunrise-xray list            # 列出所有节点（同义词：--list / ls）
sunrise-xray proxy on        # 输出 export 代理环境变量（eval "$(...)" 用）
sunrise-xray proxy off       # 输出 unset 清理代理环境变量（eval "$(...)" 用）
sunrise-xray                 # 前台跑（Ctrl+C 停）
```

| 场景 | 命令 |
|------|------|
| 刚装好，想用 | `export SUNRISE_SUB_URL='...' && sunrise-xray use` |
| 日常切节点 | `sunrise-xray use` |
| 健康检查 + 自动切换 | `sunrise-xray autoswitch`（配合 cron） |
| 临时关代理 | `sunrise-xray off` |
| 重新开 | `sunrise-xray on` |
| 看通不通 | `sunrise-xray test` |
| 输出 / 清理代理环境变量 | `eval "$(sunrise-xray proxy on)"` / `eval "$(sunrise-xray proxy off)"` |

---

## 🧩 默认端口

| 协议 | 地址 | 可配方式 |
|------|------|----------|
| SOCKS5 | `127.0.0.1:10808` | `--socks-port` / `SUNRISE_SOCKS_PORT` |
| HTTP | `127.0.0.1:10809` | `--http-port` / `SUNRISE_HTTP_PORT` |

---

## 📊 协议兼容性矩阵

| 协议 | TCP | WebSocket | gRPC (Gun) | gRPC (Multi) | HTTP/2 (h2) | REALITY | TLS | None |
|------|:---:|:---------:|:----------:|:------------:|:----------:|:-------:|:---:|:----:|
| **VLESS** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **VMess** | ✅ | ✅ | ✅ | ✅ | ✅ | – | ✅ | ✅ |
| **Trojan** | ✅ | ✅ | ✅ | ✅ | ✅ | – | ✅ | – |
| **Shadowsocks** | ✅ | – | – | – | – | – | – | – |

---

## 🎯 架构总览

```
┌──────────────────────────────────────────────────────────┐
│                   build.rs（编译期）                       │
│  下载 Xray release → SHA256 校验 → 嵌入二进制             │
└─────────────────────┬────────────────────────────────────┘
                      │ include_bytes!(xray.zip)
┌─────────────────────▼────────────────────────────────────┐
│               sunrise-xray 单文件（22MB）                   │
│                                                          │
│  main.rs: CLI 入口 + 子命令分派                            │
│  fetch.rs: HTTP 订阅拉取 + 指数退避重试                     │
│  config.rs: URI 解析 + 4 协议 × 5 传输层 × 3 安全层        │
│  embedded.rs: 首次运行释放 xray + geo 文件                  │
│  xray.rs: 进程生命周期管理                                  │
│  commands.rs: daemon 管理 / 交互选择 / 自愈逻辑             │
└─────────────────────┬────────────────────────────────────┘
                      │ 释放 xray 二进制 → spawn 子进程
┌─────────────────────▼────────────────────────────────────┐
│               Xray-core（子进程）                          │
│  监听 127.0.0.1:10808 (SOCKS5)                            │
│  监听 127.0.0.1:10809 (HTTP)                              │
└──────────────────────────────────────────────────────────┘
```

详细设计见 [架构文档](docs/architecture.md)。

---

## 📦 安装方式

### 方式一：一键安装（推荐）

```bash
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash
```

脚本自动完成：平台探测 → 镜像下载 → SHA256 校验 → 解压安装 → PATH 写入。

**镜像策略**（自动 fallback）：
1. Qiniu CDN — 大陆访问最快
2. ghproxy.net / gh-proxy.com — GitHub 加速
3. 直连 GitHub Release

每个镜像 15 秒连接超时 + 50 KB/s 速度下限，慢就自动换下一个。

**常用参数**：

```bash
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash -s -- \
  --version v0.3.4           # 指定版本
  --dir /usr/local/bin       # 安装目录
  --mirror https://my-cdn.example.com  # 自建镜像
  --no-path-update           # 不自动写 shell rc
```

### 方式二：手动下载

| 文件 | 适用平台 |
|------|----------|
| `sunrise-xray-*-x86_64-apple-darwin.tar.gz` | Intel Mac |
| `sunrise-xray-*-aarch64-apple-darwin.tar.gz` | Apple Silicon Mac |
| `sunrise-xray-*-x86_64-unknown-linux-musl.tar.gz` | x86_64 Linux |
| `sunrise-xray-*-aarch64-unknown-linux-musl.tar.gz` | ARM64 Linux |
| `sunrise-xray-*-x86_64-pc-windows-msvc.zip` | 64 位 Windows |

下载地址：[GitHub Releases](https://github.com/Sunrisies/sunrise-xray/releases) · [Qiniu CDN](https://cdn.sunrise1024.top/sunrise-xray/latest.txt)

### 方式三：从源码编译

```bash
git clone https://github.com/Sunrisies/sunrise-xray.git
cd sunrise-xray
cargo build --release
export SUNRISE_SUB_URL='https://你的订阅地址'
./target/release/sunrise-xray use
```

> 首次编译需要网络（从 GitHub 下载 xray-core zip，约 30 秒~几分钟），自动多镜像重试。
> 完全离线：`SUNRISE_XRAY_ZIP=/path/to/Xray-xxx.zip cargo build --release`

详情见 [开发者指南](docs/dev-guide.md#从源码构建)。

---

## 🛡️ 安全与隐私

- **订阅 URL 自动脱敏**（v0.3.3+）：所有错误日志、调试输出中的订阅地址自动替换为 `scheme://host/***`，路径和 token 不暴露
- **编译期 SHA256 校验**：下载的 xray-core zip 通过 GitHub Release API 获取官方 digest 比对，篡改即报错
- **白名单解压**：嵌入 zip 只释放 `xray` / `xray.exe` / `geoip.dat` / `geosite.dat`，不受 zip-slip 攻击影响
- **daemon 独立会话**：通过 `setsid()` 脱离终端，SSH 断开不影响代理进程
- **无网络后门**：代码量仅 ~1200 行，无第三方遥测 / 统计 / 回传

---

## 📁 项目结构

```
build.rs                  # 编译期下载 xray-core + SHA256 校验 + 嵌入
scripts/install.sh        # 一键安装脚本（多级镜像 + 自动 PATH）
.github/workflows/
├── ci.yml                # PR/push: cargo check + test
└── release.yml           # v* tag: 5 平台编译 → GitHub Release → Qiniu CDN
src/
├── main.rs               # CLI 入口 + clap 子命令分派
├── commands.rs           # daemon 管理 + 交互选择 + 自愈
├── fetch.rs              # 订阅 HTTP 拉取 + 指数退避重试
├── config.rs             # URI 解析 + xray JSON 生成（4 协议 × 5 传输层）
├── paths.rs              # XDG 路径管理（cache / pid / log / state）
├── util.rs               # 容错 base64 解码 + URL 脱敏
├── embedded.rs           # 嵌入 zip 释放 + 版本对账
└── xray.rs               # xray 子进程 spawn + 生命周期
```

---

## 🧠 技术亮点

| 设计决策 | 说明 |
|----------|------|
| **编译期嵌入 xray** | `build.rs` 下载 xray-core zip → SHA256 校验 → `include_bytes!` 嵌入二进制。运行时零网络依赖，单文件交付 |
| **容错 base64** | 自动剥离空白、兼容 URL-safe 字母（`-/`→`+/`）、智能补齐 padding，适配各类机场的"不规范"编码 |
| **并发延迟探测** | `tokio::JoinSet` 同时 connect 所有节点，3 秒超时，毫秒级排序 |
| **指数退避重试** | 订阅请求：1s → 2s 间隔，传输层错误 / 5xx / 429 重试，4xx 立即失败 |
| **脱敏而非隐藏** | 不影响调试的脱敏——保留 scheme + host + port 便于区分哪个订阅出问题，只丢弃 path/query |
| **版本对账** | `version.tag` 文件记录已释放的 xray 版本，升级 binary 后自动重新解压 |

---

## 🔗 相关资源

- [架构设计文档](docs/architecture.md) — 编译期嵌入策略、运行时流程、关键设计决策
- [开发者指南](docs/dev-guide.md) — 环境搭建、新增协议、测试、发布流程
- [部署运维指南](docs/deploy-guide.md) — launchd/systemd 托管、cron 自愈、故障排查
- [详细用法 (USAGE.md)](USAGE.md) — 子命令详解、文件清单、完全卸载
- [Xray-core 官方文档](https://xtls.github.io/)

---

## 📄 License

MIT © [Sunrisies](https://github.com/Sunrisies)。xray-core 版权归 [XTLS](https://github.com/XTLS/Xray-core) 所有。
