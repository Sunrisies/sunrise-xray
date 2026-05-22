# TODO

待办问题清单。按优先级（P0 > P1 > P2）排，每条带文件:行号定位。完成的勾掉。

---

## P0 — 阻塞性（功能 / 安全）

- [x] **#1 只能在 macOS 跑** — `macos_asset_name()` 只返回 macOS 资产名，其他系统 `bail!`。需要按 OS+ARCH 返回对应资产名（Linux x86_64 / arm64 / arm32、Windows x86_64）
  - 位置：`src/xray.rs:23-29`
- [x] **#2 下载的 xray 二进制无校验** — 从 5 个第三方镜像下任意字节当可执行文件运行，没有 SHA256/签名校验，镜像被投毒就完蛋。release JSON 里有官方 sha256，可比对
  - 位置：`src/xray.rs:144-159`
  - 备注：后续整体改为编译期 embed + SHA256 校验后，运行时下载路径已删除
- [x] **#4 只识别 VLESS+REALITY** — 订阅里的 vmess / trojan / ss / hysteria2 全被丢弃
  - 位置：`src/config.rs:22-32`
  - 备注：VLESS（REALITY/TLS/none）+ Trojan + VMess + Shadowsocks 已支持；Hysteria2 是 xray 不原生支持的协议，需要额外集成 sing-box 才能跑，独立议题

## P1 — 健壮性 / 体验

- [x] **#5 路径全写 /tmp** — 系统重启清空，Linux 多用户冲突。改 XDG（`~/.cache/`、`~/.config/`），用 `dirs` crate
  - 位置：`src/main.rs:12`、`src/xray.rs:10`
- [ ] **#6 端口硬编码 10808/10809** — 不可配。需要 env 或 CLI 参数
  - 位置：`src/main.rs:10-11`
- [x] **#7 GitHub release API 匿名调用** — 共享 IP 容易被 429。支持 `GITHUB_TOKEN` env
  - 位置：`src/xray.rs:9, 77-87`
  - 备注：xray 现在改为编译期 embed，运行时不再调 GitHub API，问题消解
- [x] **#8 `which` 命令不跨平台** — Windows 没 `which`。改用 `which` crate
  - 位置：`src/xray.rs:56-67`
  - 备注：xray 改为编译期 embed 后已删除自动发现逻辑
- [ ] **#9 xray 日志无文件落地、无轮转** — stdout/stderr 直接 inherit，长跑撑爆日志文件
  - 位置：`src/xray.rs:206-207`
- [ ] **#10 失败无重试** — 订阅请求 + 单镜像下载都是一次失败就跳，没有 backoff 重试
  - 位置：`src/fetch.rs:13-22`、`src/xray.rs:144-159`
- [ ] **#11 订阅格式探测脆弱** — `body.contains("://")` 不识别 clash YAML 等格式
  - 位置：`src/fetch.rs:25`
- [x] **#12 没 `--version` / `--help`** — 没用 `clap`，发布版本无从查询
  - 位置：`src/main.rs`
- [x] **#13 `SUNRISE_SUB_URL` 无格式校验** — 传错字符串会从 reqwest 内部冒出晦涩错误，应先 `Url::parse` 校验
  - 位置：`src/main.rs:26-28`

## P2 — 代码质量 / 规范

- [ ] **#14 零测试** — `config.rs` 的 URI 解析、`fetch.rs` 的 base64 兼容（URL-safe / padding）边界条件都没 unit test
  - 位置：全仓
- [x] **#15 `extract_xray_bin` 未显式拒绝路径遍历** — 目前靠白名单文件名兜底，没用 `entry.enclosed_name()` 校验，逻辑一变就会有 zip-slip 风险
  - 位置：`src/xray.rs:161-197`
  - 备注：解压的 zip 现在来自编译期校验过 SHA256 的可信源（GitHub release），白名单兜底仍在
- [ ] **#16 路由规则写死** — cn 直连、`domainStrategy` 都硬编码，没法配
  - 位置：`src/config.rs:156-167`
- [ ] **#17 VLESS 网络层只生成 tcp 配置** — 没有 `wsSettings`/`grpcSettings`/`httpSettings`，订阅里有 ws/grpc 节点会启动后连不通
  - 位置：`src/config.rs:80, 99-120`
- [x] **#18 `Cargo.toml` 元信息缺失** — 没 `description` / `license` / `repository` / `authors` / `rust-version`
  - 位置：`Cargo.toml`
- [x] **#19 没有 LICENSE 文件** — README 写了 MIT 但仓库根目录没 `LICENSE` 文件
  - 位置：仓库根
- [x] **#20 `user_agent` 写死 "0.1"** — 升级版本号 user-agent 还停留在旧值，应用 `env!("CARGO_PKG_VERSION")`
  - 位置：`src/xray.rs:72`、`src/fetch.rs:8`
- [ ] **#21 没有 CI** — 没 GitHub Actions，跨平台编译没人验证
  - 位置：仓库根

---

## 建议的修复顺序

前置不顺会卡住后续，按这个顺序改：

1. **#18 + #19**（Cargo 元信息 + LICENSE）— 改动小、和功能解耦
2. **#12 + #13**（clap + URL 校验）— 之后所有需要 CLI 参数的功能都基于此
3. **#5**（XDG 路径）— Linux 支持的前置
4. **#1**（多平台资产名）— 进入 Linux 支持
5. **#2**（SHA256 校验）— 安全相关，越早越好
6. **#3 + #4**（节点选择 + 多协议）— 体量较大，建议拆两个 commit / PR
7. **#14**（测试）— 在 #3 #4 改造后补，覆盖新逻辑
