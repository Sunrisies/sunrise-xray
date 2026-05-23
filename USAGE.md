# sunrise-xray 使用文档

一个把订阅自动拉成 xray 代理服务的小工具。本文档是日常运维 + 故障排查的速查。新装的用户先看 [README.md](./README.md) 的「30 秒上手」。

---

## 一、文件清单

### 二进制 + 配置（每次启动都会用）

| 文件 | 作用 |
|---|---|
| `~/.local/bin/sunrise-xray` | 主二进制（一键安装脚本默认装这里） |
| `~/Library/Caches/sunrise-xray/bin/{xray,geoip.dat,geosite.dat,version.tag}` | 首次启动从内置 zip 释放的 xray + 数据文件（Linux: `~/.cache/sunrise-xray/bin/`） |
| `~/Library/Caches/sunrise-xray/xray_config.json` | 运行时生成的 xray 配置（Linux: `~/.cache/sunrise-xray/`） |

### Daemon 模式状态文件（`sunrise-xray on` 后才有）

| 文件 | 作用 |
|---|---|
| `~/Library/Caches/sunrise-xray/sunrise-xray.pid` | 后台 xray 子进程 PID，`off`/`status` 读这个 |
| `~/Library/Caches/sunrise-xray/sunrise-xray.log` | 后台模式 xray 的 stdout/stderr，`logs` 读这个 |
| `~/Library/Caches/sunrise-xray/state.json` | 启动时选了哪个节点 / 端口 / 启动时间，`status` 用 |

### 可选：launchd 自启相关（如果你配了）

| 文件 | 作用 |
|---|---|
| `~/Library/LaunchAgents/com.sunrise-xray.proxy.plist` | 开机/登录自动起 |
| `~/.zshrc` 末尾 `# >>> sunrise-xray proxy >>>` 段 | 自动 export `http_proxy` 等 |
| `/tmp/sunrise-xray.log` | launchd 重定向的日志（**和上面 daemon 模式日志是两个不同文件**） |

### 默认端口

- **SOCKS5**：`127.0.0.1:10808`（可通过 `--socks-port` / `SUNRISE_SOCKS_PORT` 改）
- **HTTP**：`127.0.0.1:10809`（可通过 `--http-port` / `SUNRISE_HTTP_PORT` 改）

订阅源：通过 `SUNRISE_SUB_URL` 环境变量传入（shell 里 export 或 launchd plist 的 `EnvironmentVariables`）。

---

## 二、两种运行模式

`sunrise-xray` 自己内置 daemon 管理。同时也兼容你之前可能配过的 launchd 方案。两种**任选其一**，**不要同时用**（同一个端口绑两次会冲突）。

| 模式 | 命令 | 重启后自动起 | 适用场景 |
|---|---|---|---|
| **内置 daemon**（v0.3+ 起，推荐） | `sunrise-xray on` / `off` | ❌ 需手动起 | 想自己控、要频繁换节点 |
| **launchd 托管**（macOS） | plist 文件 | ✅ 登录即起 | 想"设了就忘"，电脑开机就有代理 |
| **systemd 托管**（Linux） | unit 文件 | ✅ 开机即起 | Linux 服务器 |
| **前台手动**（裸命令） | `sunrise-xray` | ❌ Ctrl+C 即停 | 临时调试 |

### 内置 daemon vs launchd 的状态文件区别

- **内置 daemon**：状态在 `~/Library/Caches/sunrise-xray/{sunrise-xray.pid, state.json}`；日志在同目录 `sunrise-xray.log`
- **launchd**：状态由 launchd 系统自己管；日志由 plist 里 `StandardOutPath` 指定（你这里是 `/tmp/sunrise-xray.log`）

两种模式启动的 xray 进程都监听 10808/10809，但**内置 daemon 写 PID 文件、launchd 不写**。所以：

- 如果你只用 launchd：`sunrise-xray status` 会说"未运行"（它只看 PID 文件），不代表真的没代理
- 想检查代理实际是否工作，用 `sunrise-xray test`（它去 connect 端口、走代理 GET 几个站）

---

## 三、日常使用

### 99% 的情况：什么都不用做

只要代理在跑（不管哪种模式），且你 `~/.zshrc` 里配了 `http_proxy`/`https_proxy` 等：

```bash
curl https://www.google.com         # 自动走 HTTP 代理
git clone https://github.com/...    # git 也走
pip install xxx                     # pip 也走
brew install xxx                    # brew 也走
```

无需 `-x` 参数。新会话默认就带。

### 内置 daemon 模式的常用命令

```bash
sunrise-xray use              # ★ 交互选节点：测延迟 → ↑↓ 选 → 自动切换 + 测试
sunrise-xray on               # 后台启动
sunrise-xray off              # 停止
sunrise-xray restart          # 重启（用同一个或新指定的节点）
sunrise-xray status           # PID / 节点 / 端口 / 运行时长
sunrise-xray test             # curl 几个站验证代理通不通
sunrise-xray logs             # 看最后 50 行日志
sunrise-xray logs -f          # tail -f
sunrise-xray logs -n 200      # 看最后 200 行
sunrise-xray list             # 列出订阅里所有节点
```

### launchd 模式的常用命令（如果你还在用）

```bash
launchctl print gui/$(id -u)/com.sunrise-xray.proxy        # 看状态
launchctl kickstart -k gui/$(id -u)/com.sunrise-xray.proxy # 重启
launchctl bootout gui/$(id -u)/com.sunrise-xray.proxy      # 临时关
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.sunrise-xray.proxy.plist  # 重启用
tail -f /tmp/sunrise-xray.log                              # 看日志
```

---

## 四、关键设计点

### 1. `no_proxy` 排除清单

```bash
no_proxy="localhost,127.0.0.1,::1,api.codemirror.codes,codemirror.codes,*.local"
```

你的 `ANTHROPIC_BASE_URL=https://api.codemirror.codes/` 走直连，不再绕香港多一跳。要加新的直连白名单，编辑 `~/.zshrc` 的 `no_proxy` 行。

### 2. `proxy off` 不会被 launchd 自动拉起

如果你用 launchd 模式，plist 里这一段：

```xml
<key>KeepAlive</key>
<dict><key>SuccessfulExit</key><false/></dict>
```

含义：**只在崩溃时重启**，干净退出（SIGINT / 主动停）不会自启。下次重启或手动 `launchctl kickstart` 才会回来。

### 3. 为什么二进制不在 `~/Desktop/`

macOS TCC（隐私保护）会拦截 launchd 访问 `~/Desktop`、`~/Documents`、`~/Downloads` 这些受保护目录。源码留在 Desktop 没问题，但 **launchd 拉起的二进制必须放 `~/.local/bin/` 这类无限制目录**，否则会卡死不输出日志。

内置 daemon 模式不走 launchd，没这个限制——但保持习惯放 `~/.local/bin/` 更省事。

---

## 五、运维操作

### 升级到最新版

```bash
# 用一键安装脚本，自带 Qiniu CDN 镜像 + 镜像 fallback + SHA256 校验
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash

# 升级后重启正在跑的代理（哪种模式跑哪种）
sunrise-xray restart                                              # 内置 daemon
# 或:
launchctl kickstart -k gui/$(id -u)/com.sunrise-xray.proxy        # launchd
```

升级到指定版本：

```bash
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash -s -- --version v0.3.1
```

### 切换节点（推荐：交互选择）

```bash
sunrise-xray use
```

这会列出所有节点 + 实测的 TCP 延迟 + 协议类型，按 ↑↓ 选，Enter 确认，自动切换 + 验证连通性。

非交互场景（脚本/cron）退回老用法：

```bash
sunrise-xray --list                       # 看节点索引
sunrise-xray --node 3 restart             # 按索引切
sunrise-xray --node 香港 restart           # 按名字子串切
```

launchd 模式下要永久绑定某个节点，改 plist `EnvironmentVariables`：

```xml
<key>EnvironmentVariables</key>
<dict>
    <key>SUNRISE_SUB_URL</key>
    <string>https://你的订阅地址</string>
    <key>SUNRISE_NODE</key>
    <string>香港</string>
</dict>
```

订阅顺序变化后索引会漂移，长期跑用名字子串更稳定。

### 切换订阅地址

**内置 daemon 模式**：直接改 shell 里 export 的 `SUNRISE_SUB_URL`，然后 `sunrise-xray restart`。

**launchd 模式**：改 plist 里 `EnvironmentVariables` 的 `SUNRISE_SUB_URL`，然后重新加载：

```bash
launchctl bootout gui/$(id -u)/com.sunrise-xray.proxy
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.sunrise-xray.proxy.plist
```

### 切换端口

默认 SOCKS5 10808 / HTTP 10809。CLI 或环境变量都行（CLI 优先）：

```bash
sunrise-xray --socks-port 1080 --http-port 1081 restart
# 或：
SUNRISE_SOCKS_PORT=1080 SUNRISE_HTTP_PORT=1081 sunrise-xray restart
```

launchd 模式在 plist `EnvironmentVariables` 里加：

```xml
<key>SUNRISE_SOCKS_PORT</key>
<string>1080</string>
<key>SUNRISE_HTTP_PORT</key>
<string>1081</string>
```

改端口后记得同步更新 `~/.zshrc` 里的 `http_proxy` / `https_proxy` / `all_proxy` 环境变量。

### 看实时日志

**内置 daemon 模式**：

```bash
sunrise-xray logs -f           # 实时跟踪
sunrise-xray logs -n 200       # 看最近 200 行
```

**launchd 模式**：

```bash
tail -f /tmp/sunrise-xray.log
```

### 暂时禁用 launchd 自启

```bash
launchctl bootout gui/$(id -u)/com.sunrise-xray.proxy
```

之后重启 Mac 不会自动起。想恢复：

```bash
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.sunrise-xray.proxy.plist
```

---

## 六、故障排查

### 1. `sunrise-xray test` 失败 / 上网卡

```bash
sunrise-xray status        # 内置 daemon 在跑吗？
# 或对 launchd 模式：
launchctl print gui/$(id -u)/com.sunrise-xray.proxy 2>&1 | grep state
```

- 没在跑 → `sunrise-xray on`，或 `launchctl kickstart -k gui/$(id -u)/com.sunrise-xray.proxy`
- 在跑但 `test` 还失败 → 看日志

### 2. 守护进程起不来

```bash
sunrise-xray logs -n 50                # 内置 daemon
# 或:
tail -50 /tmp/sunrise-xray.log         # launchd
```

常见原因：

- **`SUNRISE_SUB_URL` 未设置** → shell 里 `export SUNRISE_SUB_URL=...` 或检查 launchd plist
- **订阅 URL 失效** → 机场给的链接换了，更新即可，无需重编译
- **xray 启动失败** → 通常是数据文件丢了，删 `~/Library/Caches/sunrise-xray/bin/version.tag`（或整个 `bin/` 目录），下次启动会重新释放
- **节点全挂** → `sunrise-xray use` 看延迟，换一个

### 3. Mac 重启后没自动起来（仅 launchd 模式）

macOS LaunchAgent 需要用户「登录」才会加载。如果设了密码登录、又只 SSH 不 GUI 登录：

```bash
launchctl kickstart gui/$(id -u)/com.sunrise-xray.proxy
```

第一次 SSH 进来手动跑一次即可。或者去系统设置开启**自动登录**。

### 4. 某些命令绕不开代理（想直连）

临时方式：

```bash
no_proxy='*' curl https://内网地址
# 或
curl --noproxy '*' https://内网地址
```

永久加白名单：编辑 `~/.zshrc` 的 `no_proxy` 行，加上要直连的域名。

### 5. 怀疑代理走错了

```bash
sunrise-xray test                                                   # 一键看几个站 + 出口 IP
curl -s https://api.ipify.org                                       # 走代理（如果环境变量配了）
curl -s --noproxy '*' https://api.ipify.org                         # 不走代理对比
```

### 6. `sunrise-xray status` 说"未运行"但端口被占

`status` 只检查内置 daemon 写的 PID 文件，不感知 launchd 或其他来源的进程。要判断端口实际谁在用：

```bash
lsof -i :10808 -i :10809
```

---

## 七、完全卸载

```bash
# 1. 停内置 daemon（如果用了）
sunrise-xray off

# 2. 停 launchd（如果配了）
launchctl bootout gui/$(id -u)/com.sunrise-xray.proxy
rm ~/Library/LaunchAgents/com.sunrise-xray.proxy.plist

# 3. 删二进制和缓存
rm ~/.local/bin/sunrise-xray
rm -rf ~/Library/Caches/sunrise-xray /tmp/sunrise-xray.log
# Linux 系统改为:
# rm -rf ~/.cache/sunrise-xray

# 4. 编辑 ~/.zshrc / ~/.bashrc
#    删 "Added by sunrise-xray installer" 那行 + 它下面的 export PATH 那行
#    再删 launchd 配套的 "# >>> sunrise-xray proxy >>>" 到 "# <<<" 之间内容（如果有）
```

源码目录想留就留，`rm -rf ~/Desktop/rust/sunrise-xray/` 即可全清。

---

## 八、源码结构（备忘）

```
build.rs                  # 编译期下载 xray release zip + SHA256 校验
Cross.toml                # cross 容器透传 GITHUB_TOKEN 等 env
scripts/install.sh        # 一键安装脚本
src/
├── main.rs               # CLI 入口 + 子命令分派 + 前台模式
├── commands.rs           # daemon (on/off/status) + use 交互 + test/logs
├── fetch.rs              # 拉订阅 + base64 解码
├── config.rs             # 各类 URI 解析 + xray outbound JSON（tcp/ws/grpc/http）
├── paths.rs              # 缓存路径 + PID/log/state 文件位置
├── util.rs               # 容错 base64 解码
├── embedded.rs           # 嵌入的 xray zip + 首次解压
└── xray.rs               # spawn 嵌入的 xray 进程
.github/workflows/
├── ci.yml                # push/PR 跑 cargo check + test
└── release.yml           # v* tag 触发 5 平台编译 + Release + Qiniu
```

主要依赖：`tokio` / `reqwest` / `serde_json` / `clap` / `dialoguer` / `chrono` / `dirs` / `libc`。
Build deps：`ureq` / `sha2`。

---

## 九、常用命令速查表

| 想做的事 | 命令 |
|---|---|
| 交互选节点 | `sunrise-xray use` |
| 看代理状态 | `sunrise-xray status` |
| 看出口 IP / 验证可用 | `sunrise-xray test` |
| 看实时日志 | `sunrise-xray logs -f` |
| 临时关代理 | `sunrise-xray off` |
| 重新开代理 | `sunrise-xray on` |
| 重启 daemon | `sunrise-xray restart` |
| 列出所有节点 | `sunrise-xray list` |
| 升级到最新版 | `curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh \| bash && sunrise-xray restart` |
| 编译 + 部署（开发用） | `cd ~/Desktop/rust/sunrise-xray && cargo build --release && cp target/release/sunrise-xray ~/.local/bin/ && sunrise-xray restart` |
| 临时单条命令不走代理 | `curl --noproxy '*' <url>` |
| **launchd 模式专用**：重启 | `launchctl kickstart -k gui/$(id -u)/com.sunrise-xray.proxy` |
| **launchd 模式专用**：任务详情 | `launchctl print gui/$(id -u)/com.sunrise-xray.proxy` |
| **launchd 模式专用**：日志 | `tail -f /tmp/sunrise-xray.log` |
