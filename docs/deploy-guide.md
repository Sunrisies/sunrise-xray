# sunrise-xray 部署运维指南

> 面向运维人员和高级用户。涵盖：生产环境部署方案对比、cron 自愈配置、launchd/systemd 托管、多用户场景、监控告警、故障排查 FMEA。

---

## 目录

1. [部署模式对比](#1-部署模式对比)
2. [macOS 部署：launchd 托管](#2-macos-部署launchd-托管)
3. [Linux 部署：systemd 托管](#3-linux-部署systemd-托管)
4. [自愈配置：cron + autoswitch](#4-自愈配置cron--autoswitch)
5. [多用户 / 多实例部署](#5-多用户--多实例部署)
6. [环境变量速查表](#6-环境变量速查表)
7. [监控与日志管理](#7-监控与日志管理)
8. [故障排查 FMEA](#8-故障排查-fmea)
9. [性能与安全建议](#9-性能与安全建议)
10. [升级策略](#10-升级策略)

---

## 1. 部署模式对比

| 模式 | 命令 | 开机自启 | 日志管理 | 适用场景 |
|------|------|:--------:|----------|----------|
| **内置 daemon** | `sunrise-xray on` | ❌ 手动 | `~/.cache/.../sunrise-xray.log` | 个人桌面、频繁换节点 |
| **launchd** (macOS) | plist 文件 | ✅ 登录即起 | 由 plist `StandardOutPath` 指定 | Mac 用户"设了忘" |
| **systemd** (Linux) | unit 文件 | ✅ 开机即起 | `journalctl -u sunrise-xray` | Linux 服务器 |
| **前台模式** | `sunrise-xray` | ❌ | stdout/stderr | 调试、临时用 |
| **Docker**（实验性） | 容器 | ✅ 容器策略 | docker logs | 容器化环境 |

> **推荐**：个人桌面用内置 daemon，服务器用 systemd，Mac 配合开机自启用 launchd。

---

## 2. macOS 部署：launchd 托管

### 2.1 创建 plist

`~/Library/LaunchAgents/com.sunrise-xray.proxy.plist`：

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.sunrise-xray.proxy</string>

    <key>ProgramArguments</key>
    <array>
        <string>[[请替换为实际路径]]</string>
        <string>--node</string>
        <string>香港</string>
    </array>

    <key>EnvironmentVariables</key>
    <dict>
        <key>SUNRISE_SUB_URL</key>
        <string>https://你的订阅地址</string>
        <key>SUNRISE_NODE</key>
        <string>香港</string>
    </dict>

    <key>RunAtLoad</key>
    <true/>

    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>

    <key>StandardOutPath</key>
    <string>/tmp/sunrise-xray.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/sunrise-xray.log</string>
</dict>
</plist>
```

> ⚠️ **关键配置说明**：
> - `ProgramArguments` 的首元素必须填写 `sunrise-xray` 绝对路径，可以用 `which sunrise-xray` 查到
> - `KeepAlive` 设为**只在崩溃时重启**，干净退出不自动拉起（避免 `off` 后又自己起来）
> - macOS TCC 限制：二进制不能放在 Desktop/Documents/Downloads，务必装到 `~/.local/bin/`

### 2.2 加载 / 操作

```bash
# 加载
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.sunrise-xray.proxy.plist

# 查看状态
launchctl print gui/$(id -u)/com.sunrise-xray.proxy

# 重启（-k 表示先 kill 再启动）
launchctl kickstart -k gui/$(id -u)/com.sunrise-xray.proxy

# 临时禁用（重启后生效）
launchctl bootout gui/$(id -u)/com.sunrise-xray.proxy

# 查看日志
tail -f /tmp/sunrise-xray.log
```

### 2.3 故障排查（macOS 特有）

| 现象 | 可能原因 | 解决 |
|------|----------|------|
| plist 加载成功但 xray 没起 | `ProgramArguments` 路径不对 | `which sunrise-xray` 确认路径正确 |
| 日志文件为空 | TCC 拦截 | 确保二进制不在 Desktop/Documents |
| 重启后未自动启动 | 未 GUI 登录 | `launchctl kickstart gui/$(id -u)/com.sunrise-xray.proxy` |
| 进程无限重启 | 订阅 URL 无效 | 检查 `EnvironmentVariables` 中的 `SUNRISE_SUB_URL` |

---

## 3. Linux 部署：systemd 托管

### 3.1 创建 unit 文件

`/etc/systemd/system/sunrise-xray.service`：

```ini
[Unit]
Description=sunrise-xray proxy service
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=你的用户名
ExecStart=/home/你的用户名/.local/bin/sunrise-xray --node 香港
Restart=on-failure
RestartSec=5s

Environment=SUNRISE_SUB_URL=https://你的订阅地址
Environment=SUNRISE_NODE=香港

# 安全加固
NoNewPrivileges=true
PrivateTmp=true
ProtectHome=true
ProtectSystem=strict
ReadWritePaths=/home/你的用户名/.cache/sunrise-xray

[Install]
WantedBy=multi-user.target
```

### 3.2 操作命令

```bash
# 重新加载 systemd 配置
sudo systemctl daemon-reload

# 启用开机自启
sudo systemctl enable sunrise-xray

# 启动
sudo systemctl start sunrise-xray

# 查看状态
sudo systemctl status sunrise-xray

# 查看实时日志
sudo journalctl -u sunrise-xray -f

# 重启
sudo systemctl restart sunrise-xray

# 停止
sudo systemctl stop sunrise-xray
```

### 3.3 安全加固说明

上面的 unit 文件启用了 systemd 的安全特性：

```
NoNewPrivileges=true    # 禁止通过 setuid/setcap 提权
PrivateTmp=true         # 独立的 /tmp 空间
ProtectHome=true        # 只读 home 目录
ProtectSystem=strict    # 只读大部分系统目录
ReadWritePaths=...       # 仅 sunrisix-xray 缓存目录可写
```

这确保即使 xray-core 存在未知漏洞，攻击者也无法篡改系统文件。

---

## 4. 自愈配置：cron + autoswitch

### 4.1 原理

`sunrise-xray autoswitch` 是一条 cron 友好的健康检查 + 自动故障切换命令：

```
快速路径（健康时，秒级完成）：
  ┌─ TCP connect 本地端口（验证 daemon 在跑）
  ├─ GET https://www.google.com/generate_204（验证代理通）
  └─ 204 → exit 0（健康，结束）

慢速路径（失活时，~6s）：
  ┌─ 拉取最新订阅
  ├─ 并发测试所有节点 TCP 延迟
  ├─ 选择最优活节点（排除当前故障节点）
  ├─ 生成配置 + 重启 daemon
  └─ 再次健康检查确认
```

### 4.2 配置示例

```bash
# crontab -e

# ┌── 内置 daemon 模式
*/2 * * * * /home/用户名/.local/bin/sunrise-xray autoswitch >/dev/null 2>&1

# ┌── launchd 模式（autoswitch 会自启 xray，无需再 kickstart）
*/3 * * * * /Users/用户名/.local/bin/sunrise-xray autoswitch >/dev/null 2>&1
```

### 4.3 最佳实践

| 参数 | 建议值 | 说明 |
|------|--------|------|
| 检查间隔 | `*/2`（2 分钟） | 足够短减少"断网感"，足够长不被 API 限速 |
| 超时时间 | 内建 5s | 单次健康检查请求 5s 超时 |
| node selector | 不用填 | autoswitch 自动选最优活节点 |
| 日志 | `>/dev/null 2>&1` | cron 静默，只有失败时 exit code 1 |

---

## 5. 多用户 / 多实例部署

### 5.1 多用户隔离

每个用户的 sunrise-xray 实例完全隔离（端口、缓存目录、配置文件均独立）：

```bash
# 用户 A
export SUNRISE_SUB_URL='https://sub-a.com/xxx'
sunrise-xray --socks-port 10808 --http-port 10809 on

# 用户 B
export SUNRISE_SUB_URL='https://sub-b.com/yyy'
sunrise-xray --socks-port 20808 --http-port 20809 on
```

文件隔离：
- 用户 A：`~/.cache/sunrise-xray/` + port 10808/10809
- 用户 B：`~/.cache/sunrise-xray/` + port 20808/20809

> cache 目录是按用户 home 隔离的（`dirs::cache_dir()` 返回用户级路径），所以多用户不会互相污染。

### 5.2 双节点负载均衡（高级）

一台机器跑两个实例指向不同订阅，实现主备或分流：

```bash
# 实例 1：主代理（日本节点）
SUNRISE_SUB_URL='https://sub-primary' \
  SUNRISE_SOCKS_PORT=10808 SUNRISE_HTTP_PORT=10809 \
  sunrise-xray --node 日本 on

# 实例 2：备用（新加坡节点）
SUNRISE_SUB_URL='https://sub-backup' \
  SUNRISE_SOCKS_PORT=20808 SUNRISE_HTTP_PORT=20809 \
  sunrise-xray --node 新加坡 on
```

---

## 6. 环境变量速查表

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `SUNRISE_SUB_URL` | — | **必须**。订阅地址，格式 `https://...` |
| `SUNRISE_NODE` | 第一个节点 | 节点选择，索引数字或名字子串 |
| `SUNRISE_SOCKS_PORT` | `10808` | SOCKS5 端口，`--socks-port` 优先 |
| `SUNRISE_HTTP_PORT` | `10809` | HTTP 代理端口，`--http-port` 优先 |
| `SUNRISE_MIRROR_BASE` | 空 | 安装脚本镜像基址，覆盖默认 CDN |
| `XRAY_PATH` | 嵌入的 xray | 使用外部 xray 二进制路径 |
| `http_proxy` / `https_proxy` | — | 终端代理环境变量（需手动设置） |
| `all_proxy` | — | SOCKS5 代理（需手动设置） |
| `no_proxy` | — | 代理排除列表（需手动设置） |

### shell 代理配置推荐

```bash
# 加到 ~/.zshrc 或 ~/.bashrc
export http_proxy=http://127.0.0.1:10809
export https_proxy=http://127.0.0.1:10809
export all_proxy=socks5://127.0.0.1:10808
export no_proxy="localhost,127.0.0.1,::1,*.local,内网域名"
```

---

## 7. 监控与日志管理

### 7.1 文件清单

```
~/.cache/sunrise-xray/               # macOS/ Linux 缓存目录
├── bin/
│   ├── xray                     # 释放的 xray 二进制
│   ├── geoip.dat                # IP 地理数据
│   ├── geosite.dat              # 域名分类数据
│   └── version.tag              # 已释放版本标记
├── xray_config.json             # 运行时生成的 xray 配置
├── sunrise-xray.pid             # daemon 模式 PID 文件
├── sunrise-xray.log             # daemon 模式日志
└── state.json                   # daemon 状态信息
```

### 7.2 日志管理策略

| 策略 | 命令/配置 |
|------|----------|
| 查看实时日志 | `sunrise-xray logs -f` |
| 查看最近 N 行 | `sunrise-xray logs -n 200` |
| 日志轮转（手动） | `mv ~/.cache/sunrise-xray/sunrise-xray.log{,.1} && sunrise-xray restart` |
| 日志轮转（logrotate） | 见下方配置 |

**logrotate 配置（Linux）**：

`/etc/logrotate.d/sunrise-xray`：

```
/home/*/.cache/sunrise-xray/sunrise-xray.log {
    daily
    rotate 7
    compress
    missingok
    notifempty
    copytruncate
}
```

> `copytruncate` 不需要重启服务。Mac 用户可以用 `newsyslog` 类似配置。

### 7.3 健康检查 API

```bash
# 快速健康检查脚本示例（可用于监控系统）
#!/bin/bash
# check_sunrise_xray.sh

# 1. 检查进程
if ! pgrep -f "xray.*run" > /dev/null 2>&1; then
    echo "CRITICAL: xray process not running"
    exit 2
fi

# 2. 检查端口
if ! curl -s -o /dev/null --connect-timeout 2 socks5://127.0.0.1:10808; then
    echo "WARNING: SOCKS5 port unreachable"
    exit 1
fi

# 3. 检查代理连通性
HTTP_PROXY=http://127.0.0.1:10809 \
curl -s -o /dev/null -w "%{http_code}" \
  --connect-timeout 5 \
  https://www.google.com/generate_204 | grep -q 204 || {
    echo "CRITICAL: proxy not working (no 204 from Google)"
    exit 2
}

echo "OK: sunrise-xray is healthy"
exit 0
```

集成到 Prometheus + Alertmanager：

```yaml
# 在 prometheus.yml 或 blackbox exporter 配置
- targets:
  - 'https://www.google.com/generate_204'
  - 'socks5://127.0.0.1:10808'
```

---

## 8. 故障排查 FMEA

### FMEA 矩阵

| 症状 | 概率 | 影响 | 根因 | 快速修复 | 长期方案 |
|------|:----:|:----:|------|----------|----------|
| 代理不通，test 全部失败 | ★★★ | 严重 | 节点挂了 / 被墙 | `sunrise-xray use` 换节点 | 配置 cron autoswitch |
| 代理时通时断 | ★★☆ | 中 | 节点质量差 / 网络抖动 | `sunrise-xray use` 选延迟最低 | 给 autoswitch 加通知 |
| `sunrise-xray on` 失败 | ★☆☆ | 严重 | PID 文件残留 | `sunrise-xray off` 再试 | — |
| 端口被占用 | ★★☆ | 中 | 另一个代理在跑 / 进程残留 | `lsof -i :10808` 查谁在用 | `sunrise-xray off` 清理 |
| 日志出现"base64 decode 失败" | ★★☆ | 低 | 机场换了订阅格式 | 更新 `SUNRISE_SUB_URL` | 自动 fallback |
| 订阅更新后节点变少 | ★☆☆ | 低 | 机场调整线路 | `sunrise-xray list` 确认 | 用名字子串而非索引选节点 |
| macOS 升级后 launchd 失效 | ★☆☆ | 中 | plist 缓存失效 | `launchctl bootout` + `bootstrap` | 升级后重启 |
| 磁盘写满 | ★☆☆ | 低 | 日志未轮转 | 手动轮转或清空 | 配置 logrotate |

### 详细排查步骤

#### 8.1 "代理突然不通了"

```bash
# 第 1 步：确认 daemon 在跑
sunrise-xray status

# 第 2 步：确认端口在监听
lsof -i :10808 -i :10809

# 第 3 步：本地测试代理
sunrise-xray test

# 第 4 步：看日志找线索
sunrise-xray logs -n 50

# 第 5 步：换节点试试
sunrise-xray use

# 第 6 步：还是不行就重启
sunrise-xray restart
```

#### 8.2 "安装脚本下载失败"

```bash
# 方案 A：指定镜像基址
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash -s -- \
  --mirror https://ghproxy.net

# 方案 B：从 GitHub 直接拉脚本
curl -fsSL https://raw.githubusercontent.com/Sunrisies/sunrise-xray/main/scripts/install.sh | bash

# 方案 C：手动下载
# 到 https://github.com/Sunrisies/sunrise-xray/releases 下载对应平台 tar.gz
tar -xzf sunrise-xray-*.tar.gz
install -m 0755 sunrise-xray ~/.local/bin/
```

#### 8.3 "xray 启动后立刻退出"

```bash
# 查看错误日志
sunrise-xray logs -n 50

# 常见原因：
# 1. 订阅 URL 不对 → export SUNRISE_SUB_URL
# 2. 数据文件损坏 → rm -rf ~/.cache/sunrise-xray/bin/ && sunrise-xray restart
# 3. 配置语法错误 → cat ~/.cache/sunrise-xray/xray_config.json | jq .
```

#### 8.4 "进程残留 / 端口冲突"

```bash
# 查什么占用了端口
lsof -i :10808

# 查出进程 PID 后
kill <PID>
sunrise-xray off    # 清理 PID 文件

# 或者用 fuser
fuser -k 10808/tcp
```

---

## 9. 性能与安全建议

### 9.1 性能调优

| 项 | 建议 | 原因 |
|----|------|------|
| 节点选择 | 优先选延迟 <200ms | TCP 延迟直接影响页面加载首字节时间 |
| cron 间隔 | 2–5 分钟 | 太频繁浪费 API，太疏"断网感"强 |
| 端口 | 避免用 1080/8080 等常见端口 | 减少被扫描 / 误占用的概率 |
| 内存 | xray-core 通常 ~50MB RSS | 无需额外配置 |
| 日志级别 | xray 默认 `warning` | debug 级别日志量巨大，只在排错时开启 |

### 9.2 安全加固

| 措施 | 说明 |
|------|------|
| 订阅 URL 脱敏 | v0.3.3+ 自动脱敏错误日志中的 URL，但不影响 shell history 和 plist |
| 文件权限 | `install.sh` 用 `install -m 0755` 安装，其他用户不可写 |
| systemd 加固 | 参照上一节的 `NoNewPrivileges` / `ProtectSystem` |
| 密钥管理 | 订阅 token 通过环境变量传入而非硬编码，避免提交到 git |
| 网络安全 | SOCKS5/HTTP 只监听 127.0.0.1，不暴露到局域网 |

> ⚠️ **关于订阅 URL 的安全说明**：`SUNRISE_SUB_URL` 本身在你的 shell 历史、launchd plist、环境变量里是明文。脱敏只保护「把日志截屏给别人看时不泄露 token」的场景，不能替代对主机本身的安全管控。

---

## 10. 升级策略

### 10.1 内置 daemon 升级

```bash
# 升级二进制
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash

# 重启 daemon
sunrise-xray restart
```

### 10.2 launchd 升级

```bash
# 1. 停 launchd
launchctl bootout gui/$(id -u)/com.sunrise-xray.proxy

# 2. 升级二进制
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash

# 3. 重新加载 launchd
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.sunrise-xray.proxy.plist
```

### 10.3 systemd 升级

```bash
# 1. 升级二进制
curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh | bash

# 2. 重启服务
sudo systemctl restart sunrise-xray
```

### 10.4 升级注意事项

- 升级后嵌入式 xray 版本可能会更新，首次启动会重新解压（自动触发）
- 如果升级后运行异常：先 `sunrise-xray off` 然后 `rm -rf ~/.cache/sunrise-xray/bin/`，再 `sunrise-xray on`
- CI/CD 发布的每个版本都有 SHA256 校验，安装脚本自动验证
- 建议升级前查看 [GitHub Releases](https://github.com/Sunrisies/sunrise-xray/releases) 的 release notes 了解兼容性变更

---

## 附录：常用运维命令速查

| 场景 | 命令 |
|------|------|
| 交互选节点 | `sunrise-xray use` |
| 健康检查 + 自动切换 | `sunrise-xray autoswitch` |
| 看代理状态 | `sunrise-xray status` |
| 看出口 IP | `sunrise-xray test` |
| 看实时日志 | `sunrise-xray logs -f` |
| 临时关代理 | `sunrise-xray off` |
| 重新开 | `sunrise-xray on` |
| 重启 daemon | `sunrise-xray restart` |
| 列出所有节点 | `sunrise-xray list` |
| 按名字切节点 | `sunrise-xray --node 香港 restart` |
| 升级到最新版 | `curl -fsSL https://cdn.sunrise1024.top/sunrise-xray/install.sh \| bash && sunrise-xray restart` |
| 清理缓存（重置） | `sunrise-xray off && rm -rf ~/.cache/sunrise-xray` |
| 查端口谁在用 | `lsof -i :10808 -i :10809` |
| 临时跳过代理（curl） | `curl --noproxy '*' https://内网地址` |
| macOS: 看 launchd 状态 | `launchctl print gui/$(id -u)/com.sunrise-xray.proxy` |
| macOS: 看 launchd 日志 | `tail -f /tmp/sunrise-xray.log` |
| Linux: 看 systemd 状态 | `sudo systemctl status sunrise-xray` |
| Linux: 看 systemd 日志 | `sudo journalctl -u sunrise-xray -f` |
