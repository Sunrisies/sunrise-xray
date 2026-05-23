# sunrise-xray

把订阅链接自动拉成本地 Xray 代理服务的 Rust 小工具。**自带 xray 二进制**，编译产物就是单文件，丢哪都能跑。开机自启、SSH 进来 `curl/git/pip` 直接通。

## 功能

- 拉取订阅 → base64 解码 → 解析 VLESS / VMess / Trojan / Shadowsocks 节点
- 编译期下载并嵌入 xray-core 二进制 + geoip/geosite 数据文件，运行时直接释放使用
- 生成 Xray 配置并启动进程，监听本地 SOCKS5 / HTTP 端口

默认端口：

- SOCKS5：`127.0.0.1:10808`
- HTTP：`127.0.0.1:10809`

可通过 `--socks-port` / `--http-port` 或 `SUNRISE_SOCKS_PORT` / `SUNRISE_HTTP_PORT` 环境变量修改（CLI 优先）。

## 一键安装（推荐）

最简单的方式，全平台一条命令。脚本会自动探测系统/架构、按「Qiniu CDN → ghproxy → 直连 GitHub」镜像优先级下载、SHA256 校验、装到 `~/.local/bin/sunrise-xray`：

```bash
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash
```

支持平台：

| 平台 | 架构 |
|---|---|
| macOS | x86_64（Intel）、arm64（Apple Silicon） |
| Linux | x86_64、aarch64（musl 静态链接，发行版无关） |
| Windows | 暂时只提供 zip 手动解压，见下方「手动下载」 |

### 常用参数

参数写在 `bash` 之后，用 `-s --` 分隔：

```bash
# 装特定版本
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash -s -- --version v0.1.2

# 改安装目录（例如系统级）
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash -s -- --dir /usr/local/bin

# 临时指定镜像基址（自建镜像或调试用）
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash -s -- --mirror https://my-mirror.example.com
```

### 镜像策略

`install.sh` 按以下顺序尝试，第一个能下载且 SHA256 校验通过的胜出：

1. **Qiniu CDN**：`https://cdn.sunrise1024.top/sunrise-xray/...`（大陆访问最快）
2. **GitHub 加速**：`https://ghproxy.net/` 和 `https://gh-proxy.com/` 前缀
3. **直连 GitHub Release**

境外用户也能直接用上面那条命令——Qiniu CDN 在全球都有可达性，慢就慢一点；如果完全不通就 fallback 到 GitHub。或者直接从 GitHub raw 拉脚本：

```bash
curl -fsSL https://raw.githubusercontent.com/Sunrisies/sunrise-xray/main/scripts/install.sh | bash
```

### 手动下载

预编译产物挂在两个地方：

- GitHub Releases：https://github.com/Sunrisies/sunrise-xray/releases
- Qiniu CDN：`https://cdn.sunrise1024.top/sunrise-xray/<tag>/`（`latest.txt` 是当前版本号指针）

每个产物都有同名 `.sha256` 校验文件。

## 快速开始（从源码编译）

```bash
git clone https://github.com/Sunrisies/sunrise-xray.git
cd sunrise-xray
cargo build --release
export SUNRISE_SUB_URL='https://你的订阅地址'
./target/release/sunrise-xray --list          # 看订阅里有哪些节点
./target/release/sunrise-xray                 # 用第 0 个节点启动
./target/release/sunrise-xray --node 3        # 用第 3 个节点启动
./target/release/sunrise-xray --node 香港     # 选名字含「香港」的第一个节点
./target/release/sunrise-xray --socks-port 1080 --http-port 1081   # 自定义端口
```

订阅地址通过 `SUNRISE_SUB_URL` 环境变量传入，**不要硬编码进源码**。
节点选择可通过 `--node` 参数或 `SUNRISE_NODE` 环境变量指定（CLI 优先）。
端口可通过 `--socks-port` / `--http-port` 或 `SUNRISE_SOCKS_PORT` / `SUNRISE_HTTP_PORT` 指定。

## 子命令速查

```bash
sunrise-xray                 # 前台跑（Ctrl+C 停）
sunrise-xray use             # 交互选节点：测延迟 → 上下选 → 自动切换（推荐）
sunrise-xray on              # 后台启动（同义词: start）
sunrise-xray off             # 停止后台（同义词: stop）
sunrise-xray restart         # stop + start
sunrise-xray status          # 看 PID / 节点 / 端口 / 运行时长
sunrise-xray test            # 走代理 GET 几个站点测可用性
sunrise-xray logs            # 看后台日志（默认最后 50 行）
sunrise-xray logs -f         # 持续跟踪（Ctrl+C 停）
sunrise-xray logs -n 200     # 看最后 200 行
sunrise-xray list            # 列出所有节点（同义词: --list / ls）
```

`use` 子命令会并发测每个节点的 TCP 连接延迟（3 秒超时），按延迟从小到大排序后进入交互菜单。↑↓ 移动，Enter 确认，Esc 取消。选完自动 stop 旧 daemon + 用新节点 start，最后跑一遍 test 验证连通性。

后台模式只在 Unix（Linux / macOS）上工作；Windows 直接前台跑。
状态文件位置（cache 目录里）：`sunrise-xray.pid` / `sunrise-xray.log` / `state.json`。

### 编译时

第一次编译需要网络（从 GitHub release 下 xray-core，约 30 秒~几分钟）。已经按国内常见镜像顺序自动重试，无需 VPN 也大概率能成功。

完全离线编译：先在其它机器下好对应平台的 `Xray-*.zip`，然后

```bash
SUNRISE_XRAY_ZIP=/path/to/Xray-linux-64.zip cargo build --release
```

### 跨平台部署

`cargo build --release` 自动按本机平台选择对应的 xray release（macOS x86_64/arm64、Linux x86_64/arm64/arm32/x86、Windows x86_64/arm64/x86）。最终产物是单个自包含二进制：

- macOS arm64：~22MB
- 其它平台类似量级

把它复制到目标机器即可运行，**无需另外安装 xray**。首次运行会把内置的 xray + geo 文件释放到 cache 目录（macOS: `~/Library/Caches/sunrise-xray/`、Linux: `~/.cache/sunrise-xray/`）。

## 项目结构

```
build.rs       # 编译期下载 xray release zip + SHA256 校验
src/
├── main.rs       # 主流程：拉订阅 → 解析 → 写配置 → 启 xray
├── fetch.rs      # HTTP + base64 解码订阅
├── config.rs     # 解析各类 URI + 构造 xray outbound JSON
├── paths.rs      # XDG 缓存/配置路径
├── util.rs       # 共享的容错 base64 解码
├── embedded.rs   # 嵌入的 xray zip + 首次运行解压
└── xray.rs       # 调用嵌入的 xray 进程
```

## 详细用法

部署到 launchd / systemd 自启、配置 zsh/bash 自动 export 代理环境变量、日常运维与故障排查，见 [USAGE.md](./USAGE.md)。

## 发布流程

`.github/workflows/` 下有两条流水线：

- **ci.yml** — push 到 main 或 PR 时跑 `cargo check` + `cargo test`
- **release.yml** — 推 `v*` tag 时自动多平台交叉编译，并发布到 GitHub Release + 镜像到 Qiniu CDN

发新版本：

```bash
# 1. 改 Cargo.toml 版本号
# 2. 提交
git commit -am "Release v0.2.0"
# 3. 打 tag 并推送（Action 会被触发）
git tag v0.2.0
git push origin main v0.2.0
```

Action 跑完后两个地方都有产物：

| 位置 | 路径 |
|---|---|
| GitHub Releases | https://github.com/Sunrisies/sunrise-xray/releases/tag/vX.Y.Z |
| Qiniu CDN | `https://cdn.sunrise1024.top/sunrise-xray/vX.Y.Z/` |

每个版本都打包成 5 个平台产物（每个都带 `.sha256` 校验文件）：

| 文件 | 适用平台 |
|---|---|
| `sunrise-xray-vX.Y.Z-x86_64-apple-darwin.tar.gz` | Intel Mac |
| `sunrise-xray-vX.Y.Z-aarch64-apple-darwin.tar.gz` | Apple Silicon Mac |
| `sunrise-xray-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz` | x86_64 Linux（静态链接，全发行版通用） |
| `sunrise-xray-vX.Y.Z-aarch64-unknown-linux-musl.tar.gz` | ARM64 Linux（服务器 / 树莓派 4+） |
| `sunrise-xray-vX.Y.Z-x86_64-pc-windows-msvc.zip` | 64 位 Windows |

Qiniu 镜像额外维护：

- `sunrise-xray/install.sh` — 安装脚本（CDN 域名通过 CI sed 注入 `DEFAULT_MIRROR_BASE`）
- `sunrise-xray/latest.txt` — 当前最新版本号指针（`install.sh` 不带 `--version` 时 GET 这个解析 latest）

也支持在 Actions 页面手动触发（workflow_dispatch）做 dry-run，不会创建 Release 或上传 Qiniu。

预发布版本（tag 含 `-`，如 `v0.2.0-rc1`）会自动标记为 prerelease。

## License

MIT
