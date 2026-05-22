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

## 一键安装（预编译二进制）

不想编译就用这条。脚本会自动探测平台、下载对应产物、SHA256 校验、装到 `~/.local/bin/sunrise-xray`：

```bash
# 境外 / 有梯子
curl -fsSL https://raw.githubusercontent.com/Sunrisies/sunrise-xray/main/scripts/install.sh | bash

# 大陆（七牛 CDN 镜像，待 CDN 域名配置后填入下方 <your-cdn>）
curl -fsSL https://<your-cdn>/sunrise-xray/install.sh | bash
```

常用参数（写在 `bash` 之后用 `-s --` 分隔）：

```bash
# 装特定版本
curl -fsSL https://.../install.sh | bash -s -- --version v0.1.0

# 改安装目录
curl -fsSL https://.../install.sh | bash -s -- --dir /usr/local/bin

# 临时覆盖镜像基址
curl -fsSL https://.../install.sh | bash -s -- --mirror https://my-mirror.example.com
```

支持平台：macOS x86_64 / arm64，Linux x86_64 / aarch64。Windows 暂时只提供 zip 手动解压。

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
- **release.yml** — 推 `v*` tag 时自动多平台交叉编译并发布到 GitHub Release

发新版本：

```bash
# 1. 改 Cargo.toml 版本号
# 2. 提交
git commit -am "Release v0.2.0"
# 3. 打 tag 并推送（Action 会被触发）
git tag v0.2.0
git push origin main v0.2.0
```

Action 跑完会在仓库的 Releases 页面挂上 5 个产物（每个都带 `.sha256` 校验文件）：

| 文件 | 适用平台 |
|---|---|
| `sunrise-xray-vX.Y.Z-x86_64-apple-darwin.tar.gz` | Intel Mac |
| `sunrise-xray-vX.Y.Z-aarch64-apple-darwin.tar.gz` | Apple Silicon Mac |
| `sunrise-xray-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz` | x86_64 Linux（静态链接，全发行版通用） |
| `sunrise-xray-vX.Y.Z-aarch64-unknown-linux-musl.tar.gz` | ARM64 Linux（服务器 / 树莓派 4+） |
| `sunrise-xray-vX.Y.Z-x86_64-pc-windows-msvc.zip` | 64 位 Windows |

也支持在 Actions 页面手动触发（workflow_dispatch）做 dry-run，不会创建 Release。

预发布版本（tag 含 `-`，如 `v0.2.0-rc1`）会自动标记为 prerelease。

## License

MIT
