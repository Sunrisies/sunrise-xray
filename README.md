# sunrise-xray

把订阅链接自动拉成本地 Xray 代理服务的 Rust 小工具。**自带 xray 二进制**，编译产物就是单文件，丢哪都能跑。开机自启、SSH 进来 `curl/git/pip` 直接通。

## 功能

- 拉取订阅 → base64 解码 → 解析 VLESS / VMess / Trojan / Shadowsocks 节点
- 编译期下载并嵌入 xray-core 二进制 + geoip/geosite 数据文件，运行时直接释放使用
- 生成 Xray 配置并启动进程，监听本地 SOCKS5 / HTTP 端口

默认端口：

- SOCKS5：`127.0.0.1:10808`
- HTTP：`127.0.0.1:10809`

## 快速开始

```bash
git clone https://github.com/Sunrisies/sunrise-xray.git
cd sunrise-xray
cargo build --release
export SUNRISE_SUB_URL='https://你的订阅地址'
./target/release/sunrise-xray --list          # 看订阅里有哪些节点
./target/release/sunrise-xray                 # 用第 0 个节点启动
./target/release/sunrise-xray --node 3        # 用第 3 个节点启动
./target/release/sunrise-xray --node 香港     # 选名字含「香港」的第一个节点
```

订阅地址通过 `SUNRISE_SUB_URL` 环境变量传入，**不要硬编码进源码**。
节点选择可通过 `--node` 参数或 `SUNRISE_NODE` 环境变量指定（CLI 优先）。

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

## License

MIT
