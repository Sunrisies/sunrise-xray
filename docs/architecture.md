# sunrise-xray 系统架构设计

> **版本**: v0.3.4+ · **语言**: Rust (Edition 2021) · **运行时**: Tokio async runtime

---

## 目录

1. [设计哲学](#1-设计哲学)
2. [总体架构](#2-总体架构)
3. [编译期构建流程](#3-编译期构建流程)
4. [运行时架构](#4-运行时架构)
5. [关键模块详解](#5-关键模块详解)
6. [进程生命周期](#6-进程生命周期)
7. [关键设计决策](#7-关键设计决策)
8. [安全性设计](#8-安全性设计)

---

## 1. 设计哲学

### 1.1 核心原则

| 原则 | 体现 |
|------|------|
| **单文件交付** | 编译期将 xray-core 二进制 + 数据文件嵌入产物，运行时零外部依赖 |
| **用户掌控** | 不写守护进程、不搞自启动安装、不后台驻留——用户明确调用 `on` 才后台运行 |
| **容错优先** | 网络抖动、订阅格式不规则、节点失效——每层都有优雅降级而非崩溃 |
| **隐私内建** | 订阅 URL 自动脱敏、错误日志不暴露 token，安全不是事后补丁 |

### 1.2 为什么不 XX？

> **为什么不直接用 xray-core + 配置文件？**
>
> xray-core 是通用代理引擎，不处理订阅解析、节点切换、延迟测试。你需要另外写 shell 脚本管理订阅更新、写 Python 解析 base64、写 cron 做健康检查。sunrise-xray 把这些"最后一公里"全包了，最终交付物是一个可执行文件。
>
> **为什么不做成 GUI 应用？**
>
> CLI 工具可以 SSH 远程使用、可以配 crontab 自动化、可以嵌入 CI/CD 流水线。GUI 在这些场景下反而需要额外 effort。终端用户用 `sunrise-xray use` 交互菜单已经足够直观。

---

## 2. 总体架构

```
┌──────────────────────────────────────────────────────────────────┐
│                         Build Phase (build.rs)                    │
│                                                                   │
│  GitHub Releases API ──► 下载 Xray-core zip ──► SHA256 校验       │
│                              │                                    │
│                              ▼                                    │
│                    include_bytes!("xray.zip")                      │
│                    include_str!("xray_version.txt")                │
└──────────────────────────┬───────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────────────┐
│                      Runtime (sunrise-xray binary)                │
│                                                                   │
│  ┌─────────────┐  ┌───────────┐  ┌──────────────┐               │
│  │  CLI Parser │─►│  Command  │─►│  Daemon Mgmt │               │
│  │  (clap)     │  │  Dispatch │  │  (commands)  │               │
│  └─────────────┘  └─────┬─────┘  └──────┬───────┘               │
│                         │               │                        │
│                         ▼               ▼                        │
│  ┌──────────────────────────────────────────────────┐            │
│  │             Core Pipeline                        │            │
│  │                                                   │            │
│  │  fetch_subscription() → parse_subscription()      │            │
│  │  → pick_node() → build_local_config()             │            │
│  │  → ensure_xray() → spawn xray subprocess          │            │
│  └──────────────────────────────────────────────────┘            │
│                                                                   │
│  ┌──────────────────────────────────────────────────┐            │
│  │             Support Modules                      │            │
│  │  HTTP Client  │ base64 Decoder │ URI Parser      │            │
│  │  XDG Paths    │ PID/State I/O  │ Select/Menu     │            │
│  └──────────────────────────────────────────────────┘            │
└──────────────────────────┬───────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────────────┐
│                      Xray-core (subprocess)                      │
│                                                                   │
│  SOCKS5: 127.0.0.1:10808    HTTP: 127.0.0.1:10809               │
│  Routing: geoip:cn → direct, geosite:cn → direct                 │
└──────────────────────────────────────────────────────────────────┘
```

---

## 3. 编译期构建流程

### 3.1 build.rs 下载策略

```
build.rs 被 Cargo 调用
    │
    ├── 检查 OUT_DIR/xray.zip + version.txt 是否已存在且有效
    │   └── 存在 → 跳过（rerun-if-changed 确保只在相关输入变更时重跑）
    │
    ├── 检查环境变量 SUNRISE_XRAY_ZIP（离线逃生口）
    │   └── 已设置 → 直接使用本地 zip，version tag = "vendored"
    │
    ├── pick_asset_name() 确定平台资产名
    │   └── 匹配 (os, arch) → 如 "Xray-linux-64.zip"
    │
    ├── fetch_release_zip()
    │   ├── GET api.github.com/repos/XTLS/Xray-core/releases/latest
    │   │   └── 支持 GITHUB_TOKEN / GH_TOKEN（CI 友好）
    │   ├── 从 release JSON 提取 tag_name + assets[] 中 asset 的 browser_download_url
    │   └── 读取 digest 字段（格式 "sha256:..."）
    │
    ├── download_with_mirrors()
    │   ├── ghproxy.net 代理
    │   ├── gh-proxy.com 代理
    │   ├── ghps.cc 代理
    │   ├── hub.gitmirror.com 代理
    │   └── 直连 GitHub
    │
    ├── SHA256 校验：计算下载内容的 sha256，与 release JSON 中的 digest 比对
    │   └── 不匹配 → 报错退出（防止镜像被投毒）
    │
    └── 写入 OUT_DIR/xray.zip + OUT_DIR/xray_version.txt
```

**镜像 fallback 逻辑**：

```
for prefix in [ghproxy, gh-proxy, ghps, gitmirror, ""]:
    url = prefix + github_release_url
    try_get_bytes(url, timeout=60s)
    if success and sha256 match:
        return bytes
    else:
        log warning, try next
```

### 3.2 嵌入机制

```rust
// embedded.rs
pub const XRAY_ZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/xray.zip"));
pub const XRAY_VERSION: &str = include_str!(concat!(env!("OUT_DIR"), "/xray_version.txt"));
```

- `include_bytes!` 在编译时把 zip 字节直接链接进 `.rodata` 段
- 产物体积 ≈ 自身代码（~5MB debug） + xray zip（~10-17MB 按平台）≈ 最终 15-22MB
- 运行时无需网络即可释放 xray，实现"单文件、离线可用"

### 3.3 跨平台支持

| 目标平台 | 下载的 xray 资产名 |
|----------|-------------------|
| macOS aarch64 (Apple Silicon) | `Xray-macos-arm64-v8a.zip` |
| macOS x86_64 (Intel) | `Xray-macos-64.zip` |
| Linux x86_64 | `Xray-linux-64.zip` |
| Linux aarch64 | `Xray-linux-arm64-v8a.zip` |
| Linux arm32 v7a | `Xray-linux-arm32-v7a.zip` |
| Linux x86 (32-bit) | `Xray-linux-32.zip` |
| Windows x86_64 | `Xray-windows-64.zip` |
| Windows aarch64 | `Xray-windows-arm64-v8a.zip` |
| Windows x86 (32-bit) | `Xray-windows-32.zip` |

---

## 4. 运行时架构

### 4.1 启动流程（`sunrise-xray use` 完整路径）

```
sunrise-xray use
    │
    ├── [1/3] fetch_subscription()
    │   ├── 读取 SUNRISE_SUB_URL 环境变量
    │   ├── Url::parse 校验格式（scheme 必须 http/https）
    │   ├── HTTP GET 订阅地址（30s 超时）
    │   │   └── 指数退避重试：0s → 1s → 3s（最多 3 次）
    │   │       ├── 传输层错误 → 重试
    │   │       ├── HTTP 5xx / 429 → 重试
    │   │       └── HTTP 4xx (非 429) → 立即失败
    │   └── base64 解码（容错模式）
    │
    ├── [2/3] parse_subscription()
    │   ├── 逐行遍历
    │   ├── scheme 分派：vless / trojan / ss / vmess
    │   │   ├── parse_vless_uri() → VLESS outbound JSON
    │   │   ├── parse_trojan_uri() → Trojan outbound JSON
    │   │   ├── parse_ss_uri() → Shadowsocks outbound JSON
    │   │   └── parse_vmess_uri() → VMess outbound JSON
    │   └── build_stream_settings() 按 type/security 字段分派
    │       ├── type=tcp → 无额外 settings
    │       ├── type=ws → wsSettings { path, headers.Host }
    │       ├── type=grpc → grpcSettings { serviceName, multiMode }
    │       ├── type=http/h2 → httpSettings { path, host[] }
    │       ├── security=tls → tlsSettings { serverName, fingerprint, alpn }
    │       └── security=reality → realitySettings { publicKey, shortId, serverName }
    │
    ├── 并发测延迟（measure_latencies）
    │   ├── tokio::JoinSet 同时 connect 每个节点
    │   ├── 3 秒超时，超时标记为 None
    │   └── 按 (是否超时, 延迟毫秒) 排序
    │
    ├── [3/3] 交互菜单
    │   ├── dialoguer::Select 显示排序后的节点列表
    │   ├── 用户 ↑↓ 选择 + Enter 确认
    │   ├── 生成 xray_config.json → build_local_config()
    │   ├── ensure_xray() → 释放嵌入的 xray + geo 文件
    │   ├── cmd_stop() → 停掉旧 daemon
    │   ├── spawn_xray_detached() → setsid + 新 daemon
    │   └── cmd_test() → 验证新节点连通性
    │
    └── 完成
```

### 4.2 两种运行模式

```
┌──────────────────┐     ┌───────────────────┐
│   Foreground     │     │   Daemon (后台)    │
│                  │     │                    │
│  main.rs:        │     │  commands.rs:      │
│  run_xray()       │     │  spawn_xray_detached│
│  tokio::select!  │     │  │                  │
│  ├─ child.wait() │     │  ├─ Command::new()  │
│  └─ ctrl_c()     │     │  ├─ pre_exec(setsid)│
│                  │     │  ├─ write pid file  │
│  Ctrl+C → kill   │     │  └─ write state    │
│  → exit          │     │                    │
│                  │     │  sunrise-xray off:  │
│                  │     │  kill(pid, SIGTERM) │
│                  │     │  → wait 2s → SIGKILL│
└──────────────────┘     └───────────────────┘
```

**前台模式**（无子命令 / `sunrise-xray`）：
- 直接在前台 spawn xray 子进程
- `tokio::select!` 同时等待子进程退出和 Ctrl+C 信号
- `kill_on_drop(true)` 保证异常退出时回收子进程
- stdout/stderr 透传到终端

**后台模式**（`sunrise-xray on`）：
- 通过 `CommandExt::pre_exec(setsid())` 让 xray 脱离父进程会话组
- 父进程退出 / SSH 断开不影响 xray
- stdout/stderr 重定向到 `sunrise-xray.log` 文件
- 写 PID 文件和 state.json（节点、端口、时间戳）
- 启动后 500ms 存活检查——xray 如果因为配置问题立刻退，给用户一个即时反馈

---

## 5. 关键模块详解

### 5.1 订阅解析引擎（`config.rs`）

```
parse_subscription(text)
    │
    ├── 逐行遍历
    ├── split_once("://") 切出 scheme
    ├── scheme 分派
    │   ├── "vless"   → parse_vless_uri()
    │   ├── "trojan"  → parse_trojan_uri()
    │   ├── "ss"      → parse_ss_uri()
    │   └── "vmess"   → parse_vmess_uri()
    │   └── 其他      → 计数跳过（如 hysteria2, socks5 等）
    │
    ├── 每个 parser 返回 ProxyNode { name, protocol, address, port, outbound }
    │
    └── 汇总返回 Vec<ProxyNode>
```

**URI 解析器详解**：

| 协议 | 解析方式 | 关键字段提取 |
|------|----------|-------------|
| **VLESS** | `url::Url` 标准解析 | UUID(username), host, port, query(type/security/pbk/sid/sni/fp/flow/host/path) |
| **Trojan** | `url::Url` + percent-decode | password(username), host, port, query(type/security/sni/host/path) |
| **Shadowsocks** | `url::Url` + base64 userinfo | method:password(userinfo base64 解码), host, port |
| **VMess** | base64 解码 → JSON 解析 | add/port/id/aid/net/tls/sni/host/path 等字段 |

**传输层构建**（`build_stream_settings`）：

```
query.type ──→ network (h2→http normalize)
query.security ──→ security (none/tls/reality)

network {
    "tcp"  → 无额外 settings
    "ws"   → wsSettings { path, headers.Host }
    "grpc" → grpcSettings { serviceName, multiMode }
    "http" → httpSettings { path, host[] }
}

security {
    "none"    → 无额外 settings
    "tls"     → tlsSettings { serverName, fingerprint, alpn, allowInsecure }
    "reality" → realitySettings { publicKey, shortId, serverName, fingerprint }
}
```

### 5.2 容错 Base64 解码器（`util.rs`）

```
decode_base64_loose(s)
    │
    ├── 1. 剥离所有空白字符（空格、换行、tab）
    ├── 2. URL-safe → 标准：'-' → '+', '_' → '/'
    ├── 3. 补齐 '=' padding：while len % 4 != 0 { push '=' }
    ├── 4. base64::STANDARD.decode()
    └── 5. String::from_utf8()
```

这个解码器解决了三大现实问题：
- **空白污染**：很多机场的订阅内容在传递过程中混入了换行和空格
- **URL-safe 字母**：部分实现在 URL 参数中传递 base64 时用 URL-safe 字母表
- **缺失 padding**：Go 的 `base64.RawStdEncoding` 不输出 `=`，Rust 的 base64 crate 要求严格的 padding

### 5.3 URL 脱敏模块（`util.rs`）

```rust
// 输入: "https://sub.example.com/api/v1/user/abc123def456?token=xyz789"
// 输出: "https://sub.example.com/***"
pub fn redact_url(s: &str) -> String;

// 在任意字符串中替换已知 URL 为脱敏版本
// 用于 reqwest 错误消息中的 URL 清理
pub fn redact_url_in(message: &str, original: &str) -> String;
```

**设计要点**：
- 保留 `scheme://host[:port]` 方便区分哪个订阅出错
- 丢弃全部 path / query / fragment（token 所在位置）
- 解析失败返回 `<invalid-url>`，不回显原文
- 全局搜索替换——reqwest 等库会把请求 URL 嵌入错误消息，我们事后扫一遍替换

### 5.4 嵌入 zip 释放器（`embedded.rs`）

```
ensure_extracted()
    │
    ├── 读 cache_dir/bin/version.tag
    ├── 比较版本 vs XRAY_VERSION
    │   ├── 匹配且 xray 二进制存在 → 直接返回（≈30MB 不重写）
    │   └── 不匹配或不存在 → 解压
    │
    └── extract_zip()
        ├── zip::ZipArchive 遍历所有 entry
        ├── 白名单：只释放 "xray"/"xray.exe" + "geoip.dat" + "geosite.dat"
        │   └── 其他文件忽略（防止 zip-slip 攻击）
        ├── Unix: set_permissions(0o755)
        └── 写 version.tag
```

**版本对账**为什么用 tag 文件而非 xray 二进制自身：
- 不能依赖 xray --version（子进程启动成本高）
- 用户可能手动替换 xray 二进制（虽然不推荐）
- tag 文件轻量、原子写入、可读文本方便调试

### 5.5 xray 进程管理器（`xray.rs`）

**前台运行**：

```rust
pub async fn run_xray(binary: &Path, config: &Path) -> Result<()> {
    let mut child = Command::new(binary)
        .arg("run").arg("-c").arg(config)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;

    tokio::select! {
        status = child.wait() => { /* 处理退出状态 */ }
        _ = signal::ctrl_c() => { /* 优雅终止 */ }
    }
}
```

**确保 xray 找到 geo 文件**：

```rust
if let Some(dir) = binary.parent() {
    if dir.join("geoip.dat").is_file() {
        cmd.env("XRAY_LOCATION_ASSET", dir);
    }
}
```

这是 xray-core 的约定——它从 `XRAY_LOCATION_ASSET` 或自身路径同目录查找 geo 数据文件。不做这个设置，xray 会因为找不到 geoip.dat 而报错退出。

### 5.6 健康检查与自愈（`commands.rs`）

```
autoswitch 流程：

1. 快速健康检查（TCP probe + HTTP GET，~2s）
   ├── connect(127.0.0.1:http_port, timeout=2s)
   │   └── 失败 → 判定不健康
   └── GET google.com/generate_204 via proxy, timeout=5s
       └── 非 204 → 判定不健康

2. 如果健康：
   └── exit 0（≈1 秒完成，cron 友好）

3. 如果不健康：
   ├── 拉订阅、解析节点
   ├── 并发测所有节点延迟（3s 超时）
   ├── 排除当前损坏节点 + 超时节点
   ├── 按延迟升序取第一个 → 最优候选
   ├── 生成新配置、停旧 daemon、spawn 新 daemon
   ├── 等 1.5s 预热
   └── 再做一次健康检查确认
       └── 即使失败也不抛错（cron 友好——避免每 2 分钟报错骚扰）
```

**为什么要用 `generate_204` 而非 `www.google.com`**：
- `generate_204` 是 Chrome 的探测端点，返回 HTTP 204 No Content，无 body
- 信号纯净——不会因为重定向、302、页面太大而误判
- 任何代理拦截 `generate_204` 都意味着完整的透明代理链路已就绪

---

## 6. 进程生命周期

```
                    ┌──────────────┐
                    │  Terminal    │
                    │  (user)      │
                    └──────┬───────┘
                           │
              ┌────────────┴────────────┐
              │  sunrise-xray (PID 1234)│
              │  clap parse → dispatch  │
              │  tokio runtime          │
              └────────────┬────────────┘
                           │
                           │ spawn (daemon)
                           │ pre_exec(setsid)
                           │ stdout→log, stdin→null
                           ▼
              ┌──────────────────────────┐
              │  xray (PID 5678)          │
              │  setsid() → new session  │
              │  XRAY_LOCATION_ASSET set │
              │                          │
              │  SOCKS5 :10808           │
              │  HTTP   :10809           │
              └──────────────────────────┘
                           │
              ┌────────────┴────────────┐
              │  Terminal 可以关闭了     │
              │  SSH 可以断了            │
              │  xray 继续跑             │
              └─────────────────────────┘

                     ... 之后 ...

              ┌──────────────────────────┐
              │  sunrise-xray off        │
              │  read PID file → PID 5678│
              │  kill(5678, SIGTERM)     │
              │  wait 2s                 │
              │  if alive: kill(5678, SIGKILL)
              │  clean PID + state files │
              └──────────────────────────┘
```

---

## 7. 关键设计决策

### 7.1 为什么不将 sunrise-xray 自身 daemonize？

核心设计原则：**sunrise-xray 是编排者，不是常驻进程**。

```
❌ 老做法： sunrise-xray fork → setsid → 自己变 daemon
    - 需要 double-fork 避免僵尸进程
    - 内存占用多一份（Rust 二进制 ~5MB）
    - 订阅更新、节点切换需要 IPC 或信号通信

✅ 现代做法：sunrise-xray spawn xray → exit
    - xray-core 是稳定成熟的常驻进程
    - sunrise-xray 每次执行完成订阅解析 + 配置生成 + 进程启动后即可退出
    - 节点切换 = kill 旧 xray + spawn 新 xray，无需任何 IPC
    - 内存占用仅 xray 一份
```

### 7.2 为什么选择编译期嵌入而非运行时下载？

| 对比维度 | 编译期嵌入 | 运行时下载 |
|----------|-----------|-----------|
| 首次启动速度 | 慢（编译需下载） | 快（首次运行下载） |
| 后续启动速度 | 快（zip 已在二进制中） | 快（缓存后） |
| 离线可用 | ✅ 是 | ❌ 需要至少一次在线 |
| 交付物 | 单文件 22MB | 单文件 5MB + 运行时下载 |
| 安全 | SHA256 校验在 build.rs，篡改即编译失败 | 运行时校验增加攻击面 |
| 版本管理 | 二进制与 xray 版本强绑定，测试充分 | 用户可能跑到旧 xray + 新配置 |

最终选择嵌入——安全性和统一交付的价值大于首次编译的等待。

### 7.3 为什么用 setsid 而非 double-fork？

Unix daemon 的传统做法是 double-fork：

```c
pid = fork();   // 第一次 fork
if (pid > 0) exit(0);  // 父进程退出
// 子进程（孤儿进程被 init 收养）
setsid();       // 新建会话
pid = fork();   // 第二次 fork
if (pid > 0) exit(0);  // 会话首进程退出
// 孙进程——永远无法再获得控制终端
```

sunrise-xray 只做一次 `pre_exec(setsid)`：

```rust
unsafe {
    cmd.pre_exec(|| {
        if libc::setsid() < 0 { return Err(...); }
        Ok(())
    });
}
```

**理由**：xray-core 本身就是一个长时间运行的网络服务进程，不需要控制终端。sunrise-xray 父进程在 spawn 完成后立即 exit，xray 自然变成孤儿进程被 init 收养。double-fork 的"保证永远不获得控制终端"对 xray 没有额外价值。

### 7.4 为什么用 `generate_204` 做健康检查？

| 端点 | 优点 | 缺点 |
|------|------|------|
| `google.com/generate_204` | 全球 CDN、小请求、204 无 body、信号纯净 | Google 在大陆不可达 |
| `www.google.com` | 普遍 | 可能 302 跳转、body 大、耗时 |
| `github.com` | 开发者常用 | 不稳定、可能被中间设备干扰 |
| `api.ipify.org` | 能看到出口 IP | 需要解析 JSON、可能限频 |

`generate_204` 是最佳信号——任何一个正常的透明代理都会拦截这个 URL 并返回 204。如果一个节点返回了 204，意味着完整的代理链路（DNS 解析 → TCP 连接 → TLS 握手 → HTTP 请求 → 代理转发 → 远端响应）全部正常工作。

---

## 8. 安全性设计

### 8.1 供应链安全

```
GitHub Release (XTLS/Xray-core)
    │ 官方 digest: sha256:abc123...
    │
    ▼
Mirror（ghproxy 等）
    │ 下载 zip + 计算 sha256 → 与 digest 比对
    │ 不匹配 → 编译失败
    │
    ▼
include_bytes!（编译期）
    │ zip 字节固化在二进制中
    │
    ▼
运行时释放
    │ 白名单文件：仅 xray / geoip.dat / geosite.dat
    └── zip-slip 攻击无效（不处理 path traversal 的 entry）
```

### 8.2 隐私保护

| 攻击面 | 防护措施 |
|--------|----------|
| 错误日志中的订阅 URL | `redact_url_in()` 全局替换为 `scheme://host/***` |
| 调试截图分享 | URL 脱敏后可以放心发到群聊或 Issue |
| SUNRISE_SUB_URL 环境变量 | 这是设计上的取舍——环境变量本身在 shell 历史中明文 |
| xray 自身日志 | 不包含订阅 URL，只有服务器地址和端口 |

### 8.3 进程隔离

- xray 作为独立子进程运行，非线程/协程——崩溃不影响 sunrise-xray
- `kill_on_drop(true)` 保证 sunrise-xray 异常退出时回收 xray
- setsid 让 xray 在独立会话中运行，不受终端信号影响

---

## 附录：依赖图

```
sunrise-xray
├── tokio          — async runtime + process + signal
├── reqwest        — HTTP 客户端（订阅拉取 + 健康检查）
├── serde_json     — xray 配置 JSON 生成
├── clap [derive]  — CLI 参数解析
├── dialoguer      — 交互式选择菜单
├── chrono         — state.json 时间戳
├── zip            — 嵌入 zip 解压
├── base64         — 订阅 base64 解码
├── url            — URI 解析
├── percent-encoding — Trojan password percent-decode
├── dirs           — XDG 路径（~/.cache 等）
├── libc [unix]    — setsid() + kill()
└── sha2 [build]   — SHA256 校验下载 zip
```

---

> **继续阅读**：[开发者指南](dev-guide.md) · [部署运维](deploy-guide.md) · [README](../README.md)
