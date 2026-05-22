use anyhow::{Context, Result};
use percent_encoding::percent_decode_str;
use serde_json::{json, Value};
use std::collections::HashMap;
use url::Url;

/// 已挑选好的 VLESS + REALITY 节点：节点别名 + 可直接塞进 Xray 的 outbound JSON。
pub struct ProxyNode {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub outbound: Value,
}

/// 从订阅文本中挑选第一个 VLESS+REALITY 节点。
/// 顺带把所有 REALITY 候选的名字打印出来，方便调试 / 后续做选择。
pub fn pick_reality_node(content: &str) -> Result<ProxyNode> {
    let mut reality_lines = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if !line.starts_with("vless://") {
            continue;
        }
        let url = match Url::parse(line) {
            Ok(u) => u,
            Err(_) => continue,
        };
        let is_reality = url
            .query_pairs()
            .any(|(k, v)| k == "security" && v == "reality");
        if is_reality {
            reality_lines.push(url);
        }
    }

    if reality_lines.is_empty() {
        anyhow::bail!("订阅中没有找到 VLESS+REALITY 节点");
    }

    println!("       发现 {} 个 REALITY 节点：", reality_lines.len());
    for (i, u) in reality_lines.iter().enumerate() {
        let name = fragment_name(u);
        let marker = if i == 0 { "*" } else { " " };
        println!("        {marker} [{i:02}] {name}");
    }

    vless_reality_to_outbound(&reality_lines[0])
}

fn fragment_name(url: &Url) -> String {
    url.fragment()
        .map(|f| percent_decode_str(f).decode_utf8_lossy().to_string())
        .unwrap_or_else(|| {
            format!(
                "{}:{}",
                url.host_str().unwrap_or(""),
                url.port().unwrap_or(0)
            )
        })
}

/// 把 `vless://uuid@host:port?security=reality&pbk=...&sid=...&sni=...&flow=...&fp=...#name`
/// 转换成 Xray outbound JSON。
fn vless_reality_to_outbound(url: &Url) -> Result<ProxyNode> {
    let id = url.username().to_string();
    anyhow::ensure!(!id.is_empty(), "VLESS URI 缺少 UUID");

    let address = url
        .host_str()
        .context("VLESS URI 缺少 host")?
        .to_string();
    let port = url.port().context("VLESS URI 缺少 port")?;

    let q: HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let network = q.get("type").cloned().unwrap_or_else(|| "tcp".into());
    let flow = q.get("flow").cloned().unwrap_or_default();
    let sni = q.get("sni").cloned().unwrap_or_default();
    let pbk = q.get("pbk").cloned().unwrap_or_default();
    let sid = q.get("sid").cloned().unwrap_or_default();
    let fp = q.get("fp").cloned().unwrap_or_else(|| "chrome".into());

    anyhow::ensure!(!pbk.is_empty(), "REALITY 节点缺少 publicKey (pbk)");

    let name = fragment_name(url);

    let mut user = json!({
        "id": id,
        "encryption": "none"
    });
    if !flow.is_empty() {
        user["flow"] = Value::String(flow);
    }

    let outbound = json!({
        "tag": "proxy",
        "protocol": "vless",
        "settings": {
            "vnext": [{
                "address": address,
                "port": port,
                "users": [user]
            }]
        },
        "streamSettings": {
            "network": network,
            "security": "reality",
            "realitySettings": {
                "serverName": sni,
                "publicKey": pbk,
                "shortId": sid,
                "fingerprint": fp,
                "show": false
            }
        }
    });

    Ok(ProxyNode {
        name,
        address,
        port,
        outbound,
    })
}

/// 基于挑好的代理出站，构造完整的本地 Xray 配置。
pub fn build_local_config(proxy_outbound: &Value, socks_port: u16, http_port: u16) -> Value {
    let inbounds = json!([
        {
            "tag": "socks-in",
            "listen": "127.0.0.1",
            "port": socks_port,
            "protocol": "socks",
            "settings": { "auth": "noauth", "udp": true, "ip": "127.0.0.1" },
            "sniffing": { "enabled": true, "destOverride": ["http", "tls"] }
        },
        {
            "tag": "http-in",
            "listen": "127.0.0.1",
            "port": http_port,
            "protocol": "http",
            "sniffing": { "enabled": true, "destOverride": ["http", "tls"] }
        }
    ]);

    let outbounds = json!([
        proxy_outbound,
        { "tag": "direct", "protocol": "freedom" },
        { "tag": "block",  "protocol": "blackhole" }
    ]);

    json!({
        "log": { "loglevel": "warning" },
        "inbounds": inbounds,
        "outbounds": outbounds,
        "routing": {
            "domainStrategy": "IPIfNonMatch",
            "rules": [
                { "type": "field", "ip":     ["geoip:private", "geoip:cn"], "outboundTag": "direct" },
                { "type": "field", "domain": ["geosite:cn"],                "outboundTag": "direct" }
            ]
        }
    })
}
