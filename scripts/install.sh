#!/usr/bin/env bash
#
# sunrise-xray installer
#
# Usage:
#   curl -fsSL https://<your-cdn>/sunrise-xray/install.sh | bash
#   curl -fsSL https://<your-cdn>/sunrise-xray/install.sh | bash -s -- --version v0.1.0
#
# Options (after `bash -s --`):
#   --version <tag>   指定版本（默认 latest，例如 v0.1.0）
#   --dir <path>      安装目录（默认 ~/.local/bin）
#   --mirror <url>    覆盖优先镜像基址（也可用 SUNRISE_MIRROR_BASE env）
#   -h, --help        帮助
#
set -euo pipefail

REPO="Sunrisies/sunrise-xray"
# 七牛 CDN 基址。配好 Qiniu 后改成你的 CDN 域名（不带尾斜杠），例如：
#   DEFAULT_MIRROR_BASE="https://cdn.example.com"
# 留空则跳过 Qiniu，仅使用 ghproxy → GitHub。
DEFAULT_MIRROR_BASE=""

VERSION="${SUNRISE_VERSION:-latest}"
INSTALL_DIR="${SUNRISE_INSTALL_DIR:-$HOME/.local/bin}"
MIRROR_BASE="${SUNRISE_MIRROR_BASE:-$DEFAULT_MIRROR_BASE}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --dir)     INSTALL_DIR="$2"; shift 2 ;;
        --mirror)  MIRROR_BASE="$2"; shift 2 ;;
        -h|--help)
            sed -n '3,18p' "$0" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *) echo "未知参数: $1" >&2; exit 1 ;;
    esac
done

# ---- helpers ----

log()  { printf '\033[36m[install]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[install]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31m[install]\033[0m %s\n' "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || die "缺少依赖: $1"
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        die "找不到 sha256sum 或 shasum"
    fi
}

detect_target() {
    local os arch
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)
    case "$os-$arch" in
        darwin-x86_64)              echo "x86_64-apple-darwin" ;;
        darwin-arm64|darwin-aarch64) echo "aarch64-apple-darwin" ;;
        linux-x86_64|linux-amd64)   echo "x86_64-unknown-linux-musl" ;;
        linux-aarch64|linux-arm64)  echo "aarch64-unknown-linux-musl" ;;
        *) die "不支持的平台: $os-$arch（已支持：macOS x86_64/arm64，Linux x86_64/aarch64）" ;;
    esac
}

# 试一组 URL，第一个成功且响应非空就返回内容
fetch_text() {
    local url
    for url in "$@"; do
        if body=$(curl -fsSL --max-time 20 "$url" 2>/dev/null); then
            if [[ -n "$body" ]]; then
                printf '%s' "$body"
                return 0
            fi
        fi
    done
    return 1
}

# 试一组 URL，第一个下载并通过 SHA256 校验的胜出
download_with_fallback() {
    local out="$1" expected_sha="$2"
    shift 2
    local url got
    for url in "$@"; do
        log "尝试下载: $url"
        if curl -fSL --max-time 180 -o "$out" "$url"; then
            got=$(sha256_of "$out")
            if [[ "$got" == "$expected_sha" ]]; then
                log "  校验通过 ($got)"
                return 0
            fi
            warn "  SHA256 不匹配（期望 $expected_sha，实际 $got），尝试下一个"
            rm -f "$out"
        else
            warn "  下载失败，尝试下一个"
        fi
    done
    return 1
}

resolve_latest_tag() {
    local urls=()
    [[ -n "$MIRROR_BASE" ]] && urls+=("$MIRROR_BASE/sunrise-xray/latest.txt")
    urls+=(
        "https://ghproxy.net/https://api.github.com/repos/$REPO/releases/latest"
        "https://gh-proxy.com/https://api.github.com/repos/$REPO/releases/latest"
        "https://api.github.com/repos/$REPO/releases/latest"
    )
    local body
    body=$(fetch_text "${urls[@]}") || die "无法解析最新版本号（所有镜像都失败）"
    # 七牛 latest.txt 直接是 tag；GitHub API 返回 JSON
    if [[ "$body" =~ ^v[0-9] ]]; then
        printf '%s' "$body" | head -1 | tr -d '\r\n '
    else
        printf '%s' "$body" | grep -oE '"tag_name":[[:space:]]*"[^"]+"' | head -1 \
            | sed 's/.*"\([^"]*\)"$/\1/'
    fi
}

# ---- main ----

need curl
need tar
need uname

TARGET=$(detect_target)
log "平台: $TARGET"

if [[ "$VERSION" == "latest" || -z "$VERSION" ]]; then
    VERSION=$(resolve_latest_tag) || die "拿不到 latest 版本"
fi
log "版本: $VERSION"

PKG="sunrise-xray-${VERSION}-${TARGET}.tar.gz"

# 镜像优先级：Qiniu CDN > ghproxy.net > gh-proxy.com > 直连 GitHub
PKG_URLS=()
SHA_URLS=()
if [[ -n "$MIRROR_BASE" ]]; then
    PKG_URLS+=("$MIRROR_BASE/sunrise-xray/$VERSION/$PKG")
    SHA_URLS+=("$MIRROR_BASE/sunrise-xray/$VERSION/$PKG.sha256")
fi
PKG_URLS+=(
    "https://ghproxy.net/https://github.com/$REPO/releases/download/$VERSION/$PKG"
    "https://gh-proxy.com/https://github.com/$REPO/releases/download/$VERSION/$PKG"
    "https://github.com/$REPO/releases/download/$VERSION/$PKG"
)
SHA_URLS+=(
    "https://ghproxy.net/https://github.com/$REPO/releases/download/$VERSION/$PKG.sha256"
    "https://gh-proxy.com/https://github.com/$REPO/releases/download/$VERSION/$PKG.sha256"
    "https://github.com/$REPO/releases/download/$VERSION/$PKG.sha256"
)

# 取期望的 SHA256
SHA_BODY=$(fetch_text "${SHA_URLS[@]}") || die "拿不到 SHA256 校验值（所有镜像都失败）"
EXPECTED_SHA=$(printf '%s' "$SHA_BODY" | awk '{print $1; exit}')
[[ -n "$EXPECTED_SHA" ]] || die "SHA256 文件内容异常: $SHA_BODY"
log "期望 SHA256: $EXPECTED_SHA"

# 下载产物
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT
TMP_PKG="$TMPDIR/$PKG"
download_with_fallback "$TMP_PKG" "$EXPECTED_SHA" "${PKG_URLS[@]}" \
    || die "所有镜像下载都失败"

# 解压
tar -xzf "$TMP_PKG" -C "$TMPDIR"
BIN_SRC=$(find "$TMPDIR" -type f -name sunrise-xray | head -1)
[[ -n "$BIN_SRC" ]] || die "压缩包里找不到 sunrise-xray 可执行文件"

# 安装
mkdir -p "$INSTALL_DIR"
DEST="$INSTALL_DIR/sunrise-xray"
if [[ -e "$DEST" ]]; then
    BACKUP="$DEST.bak.$(date +%s)"
    log "已存在的旧版本备份为: $BACKUP"
    mv "$DEST" "$BACKUP"
fi
install -m 0755 "$BIN_SRC" "$DEST"
log "已安装到: $DEST"

# PATH 检查
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        warn "$INSTALL_DIR 不在 PATH 中。请把下面这行加进 ~/.bashrc 或 ~/.zshrc："
        warn "  export PATH=\"$INSTALL_DIR:\$PATH\""
        ;;
esac

cat <<EOF

下一步：
  1) 设置订阅地址：
       export SUNRISE_SUB_URL='https://your.subscription.url'
  2) 查看节点：
       sunrise-xray --list
  3) 启动代理（默认 SOCKS5 10808 / HTTP 10809）：
       sunrise-xray
  4) 让终端走代理：
       export http_proxy=http://127.0.0.1:10809
       export https_proxy=http://127.0.0.1:10809
       export all_proxy=socks5://127.0.0.1:10808

更多用法见: https://github.com/$REPO#readme
EOF
