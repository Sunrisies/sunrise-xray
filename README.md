# sunrise-xray

把订阅链接自动拉成本地 Xray 代理服务的 Rust 小工具。**自带 xray 二进制**，编译产物就是单文件，丢哪都能跑。带交互式节点切换 + 内置 daemon 管理，开机自启、SSH 进来 `curl/git/pip` 直接通。

## 30 秒上手

```bash
# 1. 装（macOS + Linux 通用，Windows 见手动下载）
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash

# 2. 配订阅地址（一次性）
export SUNRISE_SUB_URL='https://你的订阅地址'

# 3. 交互式选节点 + 后台启动 + 自动测试
sunrise-xray use
```

之后想看状态 `sunrise-xray status`、想换节点 `sunrise-xray use`、想停 `sunrise-xray off`。

## 功能

- 拉取订阅 → base64 解码 → 解析 VLESS / VMess / Trojan / Shadowsocks 节点
- 编译期下载并嵌入 xray-core 二进制 + geoip/geosite 数据文件，运行时直接释放使用
- 生成 Xray 配置并启动进程，监听本地 SOCKS5 / HTTP 端口
- 内置 daemon 管理：`on/off/restart/status/test/logs`
- 交互式节点切换：`use` 子命令并发测延迟、上下键选择、Enter 即切

默认端口：

- SOCKS5：`127.0.0.1:10808`
- HTTP：`127.0.0.1:10809`

可通过 `--socks-port` / `--http-port` 或 `SUNRISE_SOCKS_PORT` / `SUNRISE_HTTP_PORT` 环境变量修改（CLI 优先）。

## 子命令速查

```bash
sunrise-xray use             # ★ 交互选节点：测延迟 → ↑↓ 选 → 自动切换（推荐）
sunrise-xray autoswitch      # 健康检查 + 不健康时自动切到最优活节点（cron 用）
sunrise-xray on              # 后台启动（同义词：start）
sunrise-xray off             # 停止后台（同义词：stop）
sunrise-xray restart         # stop + start
sunrise-xray status          # 看 PID / 节点 / 端口 / 运行时长
sunrise-xray test            # 走代理 GET Google/GitHub/ipify 等
sunrise-xray logs            # 看后台日志（默认最后 50 行）
sunrise-xray logs -f         # 持续跟踪（Ctrl+C 停）
sunrise-xray logs -n 200     # 看最后 200 行
sunrise-xray list            # 列出所有节点（同义词：--list / ls）
sunrise-xray                 # 前台跑（Ctrl+C 停，老的默认行为）
```

`use` 子命令会并发测每个节点的 TCP 连接延迟（3 秒超时），按延迟从小到大排序后进入交互菜单。↑↓ 移动，Enter 确认，Esc 取消。选完自动 stop 旧 daemon + 用新节点 start，最后跑一遍 test 验证连通性。

`autoswitch` 是 cron 友好版的自愈：通过当前代理 GET `https://www.google.com/generate_204`，204 就秒退（健康，不动），失败才自动切到延迟最低的活节点。配 cron 一行实现"节点挂了自动换"：

```bash
# crontab -e：每 2 分钟检查一次
*/2 * * * * /home/you/.local/bin/sunrise-xray autoswitch >/dev/null 2>&1
```

非交互场景（脚本 / cron）退回老用法：`sunrise-xray --node 香港 restart`。

后台模式（`on` / `off` / `restart`）只在 Unix（Linux / macOS）上工作；Windows 直接前台跑。
状态文件位置（cache 目录里）：`sunrise-xray.pid` / `sunrise-xray.log` / `state.json`。

## 一键安装（推荐）

最简单的方式，全平台一条命令。脚本会自动探测系统/架构、按「Qiniu CDN → ghproxy → 直连 GitHub」镜像优先级下载、SHA256 校验、装到 `~/.local/bin/sunrise-xray`，并自动追加 PATH 到对应 shell rc：

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
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash -s -- --version v0.3.3

# 改安装目录（例如系统级）
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash -s -- --dir /usr/local/bin

# 临时指定镜像基址（自建镜像或调试用）
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash -s -- --mirror https://my-mirror.example.com

# 不要自动改 shell rc
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash -s -- --no-path-update
```

### 镜像策略

`install.sh` 按以下顺序尝试，第一个能下载且 SHA256 校验通过的胜出：

1. **Qiniu CDN**：`https://cdn.sunrise1024.top/sunrise-xray/...`（大陆访问最快）
2. **GitHub 加速**：`https://ghproxy.net/` 和 `https://gh-proxy.com/` 前缀
3. **直连 GitHub Release**

每个 mirror 有连接超时 15 秒 + 速度下限 50 KB/s 持续 20 秒，慢就直接 fallback 到下一个，不会一直耗着。

境外用户也能直接用上面那条命令——Qiniu CDN 在全球都有可达性，慢就慢一点；如果完全不通就 fallback 到 GitHub。或者直接从 GitHub raw 拉脚本：

```bash
curl -fsSL https://raw.githubusercontent.com/Sunrisies/sunrise-xray/main/scripts/install.sh | bash
```

### 手动下载

预编译产物挂在两个地方：

- GitHub Releases：https://github.com/Sunrisies/sunrise-xray/releases
- Qiniu CDN：`https://cdn.sunrise1024.top/sunrise-xray/<tag>/`（`latest.txt` 是当前版本号指针）

每个产物都有同名 `.sha256` 校验文件。

## 从源码编译

```bash
git clone https://github.com/Sunrisies/sunrise-xray.git
cd sunrise-xray
cargo build --release
export SUNRISE_SUB_URL='https://你的订阅地址'
./target/release/sunrise-xray use         # 交互选节点（推荐）
./target/release/sunrise-xray --list      # 看完整节点列表
./target/release/sunrise-xray --node 3    # 按索引选第 3 个，前台跑
```

订阅地址通过 `SUNRISE_SUB_URL` 环境变量传入，**不要硬编码进源码**。

> 隐私提示（v0.3.3+）：所有错误日志里的订阅 URL 都会被脱敏成 `scheme://host/***` 形式，路径/查询/锚点丢弃。把错误截图或日志发给别人调试时不会泄露 token——但 `SUNRISE_SUB_URL` 本身在你 shell 历史 / launchd plist / 环境变量里依然是明文，那块要自己注意。

> 鲁棒性（v0.3.4+）：订阅请求会做指数退避重试（最多 3 次，1s/2s 间隔），传输层失败、5xx、429 都会重试；4xx 立即失败不浪费时间。`sunrise-xray autoswitch` cron 跑时偶发的网络抖动不再触发"伪故障"。

### 编译时网络

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
build.rs                  # 编译期下载 xray release zip + SHA256 校验
Cross.toml                # cross 容器透传 GITHUB_TOKEN 等 env
scripts/install.sh        # 一键安装脚本（mirror fallback + PATH 自动写）
src/
├── main.rs               # CLI 入口 + 子命令分派 + 前台模式
├── commands.rs           # daemon (on/off/status) + use 交互选择 + test/logs
├── fetch.rs              # HTTP + base64 解码订阅
├── config.rs             # 解析各类 URI + 构造 xray outbound JSON（tcp/ws/grpc/http）
├── paths.rs              # XDG 缓存路径 + PID/log/state 文件位置
├── util.rs               # 共享的容错 base64 解码
├── embedded.rs           # 嵌入的 xray zip + 首次运行解压
└── xray.rs               # 调用嵌入的 xray 进程
.github/workflows/
├── ci.yml                # push/PR 跑 cargo check + test
└── release.yml           # v* tag 触发 5 平台编译 + GitHub Release + Qiniu 镜像
```

主要依赖：`tokio` / `reqwest` / `serde_json` / `clap` / `dialoguer`（交互菜单）/ `chrono`（state.json 时间戳）/ `dirs` / `libc`（Unix daemon 用 setsid/kill）。
Build deps：`ureq` / `sha2`。

## 详细用法

部署到 launchd / systemd 自启、配置 zsh/bash 自动 export 代理环境变量、日常运维与故障排查，见 [USAGE.md](./USAGE.md)。

## 发布流程

`.github/workflows/` 下有两条流水线：

- **ci.yml** — push 到 main 或 PR 时跑 `cargo check` + `cargo test`
- **release.yml** — 推 `v*` tag 时自动多平台交叉编译，并发布到 GitHub Release + 镜像到 Qiniu CDN（含 CDN 缓存自动刷新）

发新版本：

```bash
# 1. 改 Cargo.toml 版本号
# 2. 提交
git commit -am "Release v0.4.0"
# 3. 打 tag 并推送（Action 会被触发）
git tag v0.4.0
git push origin main v0.4.0
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

每次发版自动调 `qshell cdnrefresh` 把 `install.sh` 和 `latest.txt` 从边缘缓存清掉，否则 Qiniu 默认 1 年 max-age 会让用户拉到陈旧版本。

也支持在 Actions 页面手动触发（workflow_dispatch）做 dry-run，不会创建 Release 或上传 Qiniu。

预发布版本（tag 含 `-`，如 `v0.4.0-rc1`）会自动标记为 prerelease。

## License

MIT
