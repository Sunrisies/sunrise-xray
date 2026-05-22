# sunrise-xray

把订阅链接自动拉成本地 Xray 代理服务的 Rust 小工具。开机自启、SSH 进来 `curl/git/pip` 直接通。

## 功能

- 拉取订阅 → base64 解码 → 解析 VLESS+REALITY 节点
- 自动下载并管理 xray-core 二进制（GitHub + 5 个镜像回退）
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

## 项目结构

```
src/
├── main.rs    # 主流程：拉订阅 → 解析 → 写配置 → 启 xray
├── fetch.rs   # HTTP + base64 解码订阅
├── config.rs  # 解析 vless:// URI + 构造 xray outbound JSON
└── xray.rs    # 查找/下载 xray + 进程管理
```

## 详细用法

部署到 launchd 自启、配置 zsh 自动 export 代理环境变量、日常运维与故障排查，见 [USAGE.md](./USAGE.md)。

## License

MIT
