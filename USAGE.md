# sunrise-xray 使用文档

一个把订阅自动拉成 xray 代理服务的小工具。配套了 launchd 自启 + zsh 自动 export 环境变量，目标是「SSH 进来 curl/git/pip 直接通」。

---

## 一、当前安装状态

| 文件 | 作用 |
|---|---|
| `~/.local/bin/sunrise-xray` | 守护进程二进制（launchd 用这个），自带 xray |
| `~/Desktop/rust/sunrise-xray/` | 源码与构建目录 |
| `~/Library/LaunchAgents/com.sunrise-xray.proxy.plist` | 开机/登录自动起 |
| `~/.zshrc` 末尾的 `# >>> sunrise-xray proxy >>>` 段 | 自动 export 环境变量 + `proxy` 函数 |
| `~/Library/Caches/sunrise-xray/bin/{xray,geoip.dat,geosite.dat,version.tag}` | 首次启动从内置 zip 释放的 xray 与数据文件（Linux 上是 `~/.cache/sunrise-xray/bin/`） |
| `~/Library/Caches/sunrise-xray/xray_config.json` | 运行时生成的 xray 配置（Linux 上是 `~/.cache/sunrise-xray/xray_config.json`） |
| `/tmp/sunrise-xray.log` | launchd 重定向的实时日志 |

端口：
- **SOCKS5**: `127.0.0.1:10808`
- **HTTP**: `127.0.0.1:10809`

订阅源：通过 `SUNRISE_SUB_URL` 环境变量传入（在 launchd plist 的 `EnvironmentVariables` 里配置）

默认节点：列表第 0 个（🇭🇰香港高速01）。

---

## 二、日常使用

### 99% 的情况：什么都不用做

SSH 进来后所有终端命令自动走代理：

```bash
curl https://www.google.com         # 自动走 HTTP 代理
git clone https://github.com/...    # git 也走
pip install xxx                     # pip 也走
brew install xxx                    # brew 也走
```

无需 `-x` 参数、无需 export，新会话默认就带。

### 偶尔会用到的命令

```bash
proxy status     # 看守护进程在不在 + 当前终端有没有 export 代理
proxy test       # curl 一下看出口 IP，应该是 16.162.x.x 之类（HK）
proxy off        # 临时关代理；launchd 不会自动拉起
proxy on         # 关了之后想重开
proxy log        # tail -f /tmp/sunrise-xray.log
```

---

## 三、关键设计点

### 1. `no_proxy` 排除了 Claude API 镜像

```bash
no_proxy="localhost,127.0.0.1,::1,api.codemirror.codes,codemirror.codes,*.local"
```

你的 `ANTHROPIC_BASE_URL=https://api.codemirror.codes/` 走直连，不再绕香港多一跳。如果以后要把别的域名加进直连白名单，编辑 `~/.zshrc` 的 `no_proxy` 行。

### 2. `proxy off` 不会被 launchd 自动拉起

plist 里设置：
```xml
<key>KeepAlive</key>
<dict><key>SuccessfulExit</key><false/></dict>
```

含义：**只在崩溃时重启**，干净退出（SIGINT / `proxy off`）后不会自启。下次重启或手动 `proxy on` 才会回来。

### 3. 为什么二进制不在 `~/Desktop/`

macOS TCC（隐私保护）会拦截 launchd 访问 `~/Desktop`、`~/Documents`、`~/Downloads` 这些受保护目录。源码留在 Desktop 没问题，但**launchd 拉起的二进制必须放 `~/.local/bin/` 这类无限制目录**，否则会卡死不输出日志。

---

## 四、运维操作

### 重启守护进程

```bash
launchctl kickstart -k gui/$(id -u)/com.sunrise-xray.proxy
```

`-k` 表示先杀再起。这条命令在改完源码 + 替换二进制后用。

### 重新编译并部署

```bash
cd ~/Desktop/rust/sunrise-xray
cargo build --release
cp target/release/sunrise-xray ~/.local/bin/sunrise-xray
launchctl kickstart -k gui/$(id -u)/com.sunrise-xray.proxy
```

### 切换订阅地址

订阅地址通过环境变量 `SUNRISE_SUB_URL` 传入。launchd 启动时需要在 plist 的 `EnvironmentVariables` 中配置：

```xml
<key>EnvironmentVariables</key>
<dict>
    <key>SUNRISE_SUB_URL</key>
    <string>https://你的订阅地址</string>
</dict>
```

改完 plist 后重新加载：

```bash
launchctl bootout gui/$(id -u)/com.sunrise-xray.proxy
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.sunrise-xray.proxy.plist
```

### 切换节点

通过 CLI 参数或环境变量选择（CLI 优先）：

```bash
sunrise-xray --list             # 列出订阅里所有节点和索引
sunrise-xray                    # 默认用第 0 个节点
sunrise-xray --node 3           # 按索引选第 3 个
sunrise-xray --node 香港        # 按名字子串匹配（大小写不敏感）
```

launchd 启动时在 plist `EnvironmentVariables` 里加 `SUNRISE_NODE`：

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

### 看实时日志

```bash
proxy log
# 等同于
tail -f /tmp/sunrise-xray.log
```

### 看 launchd 任务状态

```bash
launchctl print gui/$(id -u)/com.sunrise-xray.proxy
```

关心的字段：
- `state = running` → 在跑
- `last exit code = (never exited)` → 从启动到现在没崩
- `pid = NNNN` → 进程号

### 暂时禁用 launchd 自启

```bash
launchctl bootout gui/$(id -u)/com.sunrise-xray.proxy
```

重新启用：

```bash
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.sunrise-xray.proxy.plist
```

---

## 五、故障排查

### 1. `proxy test` 报失败 / curl 超时

```bash
proxy status
```

- **「服务：未运行」** → `proxy on` 或 `launchctl kickstart -k gui/$(id -u)/com.sunrise-xray.proxy`
- **「服务：运行中」但 curl 还是不行** → 看日志：`tail -50 /tmp/sunrise-xray.log`

### 2. 守护进程起不来

```bash
tail -50 /tmp/sunrise-xray.log
```

常见原因：
- **`SUNRISE_SUB_URL` 未设置** → 检查 launchd plist 的 `EnvironmentVariables`，或手动运行时 `export SUNRISE_SUB_URL=...`
- **订阅 URL 失效** → 网站给的链接换了，更新环境变量即可，无需重新编译
- **xray 启动失败** → 通常是数据文件丢了，删除 cache 目录的 `version.tag`（或整个 `bin/`），下次启动会重新从内置 zip 释放

### 3. 重启 Mac 后没自动起来

macOS LaunchAgent 需要用户「登录」才会加载。如果你设了密码登录、又只 SSH 不 GUI 登录：

```bash
launchctl kickstart gui/$(id -u)/com.sunrise-xray.proxy
```

第一次 SSH 进来手动跑一次即可。或者去系统设置开启**自动登录**，下次重启会自动有。

### 4. 某些命令绕不开代理（想直连）

临时方式：

```bash
no_proxy='*' curl https://内网地址
# 或
curl --noproxy '*' https://内网地址
```

永久加白名单：编辑 `~/.zshrc` 的 `no_proxy` 行，加上你要直连的域名。

### 5. 怀疑代理走错了

```bash
proxy test                                                          # 看出口 IP
curl -s https://api.ipify.org                                       # 走代理（自动）
curl -s --noproxy '*' https://api.ipify.org                         # 不走代理对比
```

---

## 六、完全卸载

```bash
# 1. 停 launchd
launchctl bootout gui/$(id -u)/com.sunrise-xray.proxy
rm ~/Library/LaunchAgents/com.sunrise-xray.proxy.plist

# 2. 删二进制和缓存
rm ~/.local/bin/sunrise-xray
rm -rf ~/Library/Caches/sunrise-xray /tmp/sunrise-xray.log
# Linux 系统改为：
# rm -rf ~/.cache/sunrise-xray

# 3. 编辑 ~/.zshrc
#    删除 "# >>> sunrise-xray proxy >>>" 到 "# <<< sunrise-xray proxy <<<" 之间所有内容
```

源码目录 `~/Desktop/rust/sunrise-xray/` 想留就留、想删就 `rm -rf`。

---

## 七、源码结构（备忘）

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

依赖：`tokio`, `reqwest`, `serde_json`, `anyhow`, `zip`, `base64`, `url`, `percent-encoding`, `clap`, `dirs`。
Build deps：`ureq`, `sha2`, `serde_json`, `anyhow`。

---

## 八、常用命令速查表

| 想做的事 | 命令 |
|---|---|
| 看代理状态 | `proxy status` |
| 看出口 IP | `proxy test` |
| 看日志 | `proxy log` |
| 临时关代理 | `proxy off` |
| 重新开代理 | `proxy on` |
| 重启守护进程 | `launchctl kickstart -k gui/$(id -u)/com.sunrise-xray.proxy` |
| 编译 + 部署 | `cd ~/Desktop/rust/sunrise-xray && cargo build --release && cp target/release/sunrise-xray ~/.local/bin/ && launchctl kickstart -k gui/$(id -u)/com.sunrise-xray.proxy` |
| 看 launchd 任务详情 | `launchctl print gui/$(id -u)/com.sunrise-xray.proxy` |
| 临时单条命令不走代理 | `curl --noproxy '*' <url>` |
