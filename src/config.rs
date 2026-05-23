use crate::util;
use anyhow::{Context, Result};
use percent_encoding::percent_decode_str;
use serde_json::{json, Value};
use std::collections::HashMap;
use url::Url;

/// 解析好的代理节点：name + protocol + 可直接塞进 Xray outbounds 的 JSON。
pub struct ProxyNode {
    pub name: String,
    pub protocol: &'static str,
    pub address: String,
    pub port: u16,
    pub outbound: Value,
}

/// 节点选择方式：默认（第一个）/ 按索引 / 按名字子串。
pub enum NodeSelector {
    Default,
    Index(usize),
    Name(String),
}

/// 解析 `--node` / `SUNRISE_NODE` 字符串：纯数字按索引，否则按名字子串匹配。
pub fn parse_selector(s: &str) -> NodeSelector {
    let s = s.trim();
    if s.is_empty() {
        return NodeSelector::Default;
    }
    if let Ok(i) = s.parse::<usize>() {
        return NodeSelector::Index(i);
    }
    NodeSelector::Name(s.to_string())
}

/// 按指定方式从节点列表中挑选一个，返回 (index, node)。
pub fn pick_node<'a>(
    nodes: &'a [ProxyNode],
    sel: &NodeSelector,
) -> Result<(usize, &'a ProxyNode)> {
    anyhow::ensure!(!nodes.is_empty(), "没有可用的节点");
    match sel {
        NodeSelector::Default => Ok((0, &nodes[0])),
        NodeSelector::Index(i) => nodes
            .get(*i)
            .map(|n| (*i, n))
            .with_context(|| format!("节点索引 {i} 超出范围（共 {} 个）", nodes.len())),
        NodeSelector::Name(needle) => {
            let needle_lc = needle.to_lowercase();
            nodes
                .iter()
                .enumerate()
                .find(|(_, n)| n.name.to_lowercase().contains(&needle_lc))
                .with_context(|| format!("没有名字包含 '{needle}' 的节点"))
        }
    }
}

/// 打印节点列表。`selected_idx` 给出会在对应行前标 `*`。
pub fn print_node_list(nodes: &[ProxyNode], selected_idx: Option<usize>) {
    println!("       共 {} 个可用节点：", nodes.len());
    for (i, n) in nodes.iter().enumerate() {
        let marker = if Some(i) == selected_idx { "*" } else { " " };
        println!(
            "        {marker} [{i:02}] {:<7} {} ({}:{})",
            n.protocol, n.name, n.address, n.port
        );
    }
}

/// 遍历订阅文本的所有行，按 URI scheme 分派到具体 parser；汇总解析结果。
/// 不能识别 / 解析失败的节点会打印一行 stderr 提示，不会让整体失败。
pub fn parse_subscription(text: &str) -> Vec<ProxyNode> {
    let mut nodes = Vec::new();
    let mut skipped_protocol: HashMap<String, usize> = HashMap::new();
    let mut skipped_reason: Vec<(String, String)> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let scheme = match line.split_once("://") {
            Some((s, _)) => s.to_lowercase(),
            None => continue,
        };

        let parsed = match scheme.as_str() {
            "vless" => parse_vless_uri(line),
            "trojan" => parse_trojan_uri(line),
            "ss" => parse_ss_uri(line),
            "vmess" => parse_vmess_uri(line),
            _ => {
                *skipped_protocol.entry(scheme).or_insert(0) += 1;
                continue;
            }
        };

        match parsed {
            Ok(n) => nodes.push(n),
            Err(e) => {
                let preview = line.chars().take(72).collect::<String>();
                skipped_reason.push((preview, format!("{e:#}")));
            }
        }
    }

    for (proto, count) in &skipped_protocol {
        eprintln!("       跳过 {count} 个 {proto}:// 节点（暂不支持的协议）");
    }
    for (uri, reason) in &skipped_reason {
        eprintln!("       跳过节点 {uri}... 原因: {reason}");
    }

    nodes
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

fn parse_vless_uri(line: &str) -> Result<ProxyNode> {
    let url = Url::parse(line).context("vless URL 解析失败")?;

    let id = url.username().to_string();
    anyhow::ensure!(!id.is_empty(), "VLESS URI 缺少 UUID");

    let address = url.host_str().context("VLESS URI 缺少 host")?.to_string();
    let port = url.port().context("VLESS URI 缺少 port")?;

    let q: HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let flow = q.get("flow").cloned().unwrap_or_default();

    let mut user = json!({ "id": id, "encryption": "none" });
    if !flow.is_empty() {
        user["flow"] = Value::String(flow);
    }

    let stream = build_stream_settings(&q, &address, "none")?;

    let name = fragment_name(&url);
    let outbound = json!({
        "tag": "proxy",
        "protocol": "vless",
        "settings": {
            "vnext": [{
                "address": address,
                "port": port,
                "users": [user],
            }],
        },
        "streamSettings": stream,
    });

    Ok(ProxyNode {
        name,
        protocol: "vless",
        address,
        port,
        outbound,
    })
}

fn parse_trojan_uri(line: &str) -> Result<ProxyNode> {
    let url = Url::parse(line).context("trojan URL 解析失败")?;

    let password = percent_decode_str(url.username())
        .decode_utf8()
        .context("trojan password 不是合法 UTF-8")?
        .to_string();
    anyhow::ensure!(!password.is_empty(), "trojan URI 缺少 password");

    let address = url.host_str().context("trojan URI 缺少 host")?.to_string();
    let port = url.port().context("trojan URI 缺少 port")?;

    let q: HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    // trojan 默认就是 tls，URI 里可以省略 security 参数
    let stream = build_stream_settings(&q, &address, "tls")?;

    let name = fragment_name(&url);
    let outbound = json!({
        "tag": "proxy",
        "protocol": "trojan",
        "settings": {
            "servers": [{
                "address": address,
                "port": port,
                "password": password,
            }],
        },
        "streamSettings": stream,
    });

    Ok(ProxyNode {
        name,
        protocol: "trojan",
        address,
        port,
        outbound,
    })
}

fn parse_ss_uri(line: &str) -> Result<ProxyNode> {
    let url = Url::parse(line).context("ss URL 解析失败")?;

    let userinfo = url.username();
    anyhow::ensure!(
        !userinfo.is_empty(),
        "SS URI 缺少 userinfo（暂不支持把 userinfo+host 整段 base64 的老格式）"
    );

    let decoded = util::decode_base64_loose(userinfo)
        .context("SS userinfo base64 解码失败")?;
    let (method, password) = decoded
        .split_once(':')
        .context("SS userinfo 解码后应为 method:password 格式")?;
    anyhow::ensure!(!method.is_empty(), "SS method 为空");
    anyhow::ensure!(!password.is_empty(), "SS password 为空");

    let address = url.host_str().context("SS URI 缺少 host")?.to_string();
    let port = url.port().context("SS URI 缺少 port")?;

    let q: HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    // SIP003 插件（obfs / v2ray-plugin / shadow-tls）在 xray 里支持有限，先拒绝
    if let Some(plugin) = q.get("plugin") {
        anyhow::bail!("暂不支持带 plugin 的 SS 节点 (plugin={plugin})");
    }

    let name = fragment_name(&url);
    let outbound = json!({
        "tag": "proxy",
        "protocol": "shadowsocks",
        "settings": {
            "servers": [{
                "address": address,
                "port": port,
                "method": method,
                "password": password,
            }],
        },
    });

    Ok(ProxyNode {
        name,
        protocol: "ss",
        address,
        port,
        outbound,
    })
}

fn parse_vmess_uri(line: &str) -> Result<ProxyNode> {
    let after_scheme = line
        .strip_prefix("vmess://")
        .context("不是 vmess:// URI")?;

    // 标准的 v2rayN VMess URI 是纯 base64，但有些客户端会在末尾带 #remark；做下兼容
    let (b64, frag) = match after_scheme.split_once('#') {
        Some((b, f)) => (b, f),
        None => (after_scheme, ""),
    };

    let json_text = util::decode_base64_loose(b64)
        .context("VMess URI base64 解码失败")?;
    let v: Value = serde_json::from_str(&json_text)
        .context("VMess URI base64 解码后不是合法 JSON")?;

    let address = field_str(&v, "add").context("VMess 缺少 add 字段")?;
    let port = field_u16(&v, "port").context("VMess port 字段非法或缺失")?;
    let id = field_str(&v, "id").context("VMess 缺少 id 字段")?;
    let alter_id = field_u32(&v, "aid").unwrap_or(0);
    let scy = field_str(&v, "scy").unwrap_or_else(|| "auto".into());

    let q = vmess_to_uri_query(&v);
    let stream = build_stream_settings(&q, &address, "none")?;

    let name = if !frag.is_empty() {
        percent_decode_str(frag).decode_utf8_lossy().to_string()
    } else {
        field_str(&v, "ps").unwrap_or_else(|| format!("{}:{}", address, port))
    };

    let outbound = json!({
        "tag": "proxy",
        "protocol": "vmess",
        "settings": {
            "vnext": [{
                "address": address,
                "port": port,
                "users": [{
                    "id": id,
                    "alterId": alter_id,
                    "security": scy,
                }],
            }],
        },
        "streamSettings": stream,
    });

    Ok(ProxyNode {
        name,
        protocol: "vmess",
        address,
        port,
        outbound,
    })
}

/// 把 VMess JSON 的字段翻译成 URI 风格的 `HashMap`，喂给 `build_stream_settings`。
fn vmess_to_uri_query(v: &Value) -> HashMap<String, String> {
    let mut q = HashMap::new();
    // (vmess_json_key, uri_query_key)
    let mapping = [
        ("net", "type"),
        ("tls", "security"),
        ("sni", "sni"),
        ("alpn", "alpn"),
        ("fp", "fp"),
        ("pbk", "pbk"),
        ("sid", "sid"),
        ("host", "host"),
        ("path", "path"),
    ];
    for (vmess_key, uri_key) in &mapping {
        if let Some(s) = v.get(*vmess_key).and_then(|x| x.as_str()) {
            if !s.is_empty() {
                q.insert((*uri_key).to_string(), s.to_string());
            }
        }
    }
    q
}

fn field_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| match x {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    })
}

fn field_u16(v: &Value, key: &str) -> Option<u16> {
    let raw = v.get(key)?;
    if let Some(n) = raw.as_u64() {
        return u16::try_from(n).ok();
    }
    if let Some(s) = raw.as_str() {
        return s.parse().ok();
    }
    None
}

fn field_u32(v: &Value, key: &str) -> Option<u32> {
    let raw = v.get(key)?;
    if let Some(n) = raw.as_u64() {
        return u32::try_from(n).ok();
    }
    if let Some(s) = raw.as_str() {
        return s.parse().ok();
    }
    None
}

/// 公共：把 URI 的 type / security 字段翻译成 xray streamSettings。
/// 支持的传输层：tcp / ws / grpc / http (含 h2 别名)。kcp / quic 等会 bail。
fn build_stream_settings(
    q: &HashMap<String, String>,
    address: &str,
    default_security: &str,
) -> Result<Value> {
    // URI 里有的写 h2 有的写 http，xray 的 network 字段统一是 http
    let raw_network = q.get("type").map(String::as_str).unwrap_or("tcp");
    let network = match raw_network {
        "h2" => "http",
        other => other,
    };

    let security_raw = q.get("security").map(String::as_str).unwrap_or("");
    let security = if security_raw.is_empty() {
        default_security
    } else {
        security_raw
    };

    let mut stream = json!({
        "network": network,
        "security": security,
    });

    // 流传输层
    match network {
        "tcp" => {}
        "ws" => {
            stream["wsSettings"] = build_ws_settings(q);
        }
        "grpc" => {
            stream["grpcSettings"] = build_grpc_settings(q);
        }
        "http" => {
            stream["httpSettings"] = build_http_settings(q);
        }
        other => anyhow::bail!("暂不支持的传输层: type={other}"),
    }

    // 安全层
    match security {
        "reality" => {
            stream["realitySettings"] = build_reality_settings(q)?;
        }
        "tls" => {
            stream["tlsSettings"] = build_tls_settings(q, address);
        }
        "none" => {}
        other => anyhow::bail!("不支持的 security: {other}"),
    }

    Ok(stream)
}

fn build_ws_settings(q: &HashMap<String, String>) -> Value {
    let path = q
        .get("path")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| "/".to_string());
    let host = q.get("host").cloned().unwrap_or_default();
    if host.is_empty() {
        json!({ "path": path })
    } else {
        json!({
            "path": path,
            "headers": { "Host": host },
        })
    }
}

fn build_grpc_settings(q: &HashMap<String, String>) -> Value {
    // 不同客户端命名不一致：serviceName / servicename / path 都见过
    let service_name = q
        .get("serviceName")
        .or_else(|| q.get("servicename"))
        .or_else(|| q.get("path"))
        .cloned()
        .unwrap_or_default();
    let mode = q.get("mode").map(String::as_str).unwrap_or("");
    json!({
        "serviceName": service_name,
        "multiMode": mode == "multi",
    })
}

fn build_http_settings(q: &HashMap<String, String>) -> Value {
    let path = q
        .get("path")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| "/".to_string());
    let host_str = q.get("host").cloned().unwrap_or_default();
    let hosts: Vec<&str> = host_str
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if hosts.is_empty() {
        json!({ "path": path })
    } else {
        json!({ "path": path, "host": hosts })
    }
}

fn build_reality_settings(q: &HashMap<String, String>) -> Result<Value> {
    let pbk = q
        .get("pbk")
        .filter(|s| !s.is_empty())
        .context("REALITY 节点缺少 publicKey (pbk)")?;
    let sni = q.get("sni").cloned().unwrap_or_default();
    let sid = q.get("sid").cloned().unwrap_or_default();
    let fp = q.get("fp").cloned().unwrap_or_else(|| "chrome".into());
    Ok(json!({
        "serverName": sni,
        "publicKey": pbk,
        "shortId": sid,
        "fingerprint": fp,
        "show": false,
    }))
}

fn build_tls_settings(q: &HashMap<String, String>, address: &str) -> Value {
    let sni = q
        .get("sni")
        .cloned()
        .unwrap_or_else(|| address.to_string());
    let fp = q.get("fp").cloned().unwrap_or_else(|| "chrome".into());
    let allow_insecure = q
        .get("allowInsecure")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let mut tls = json!({
        "serverName": sni,
        "fingerprint": fp,
        "allowInsecure": allow_insecure,
    });
    if let Some(alpn) = q.get("alpn") {
        let parts: Vec<&str> = alpn
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if !parts.is_empty() {
            tls["alpn"] = json!(parts);
        }
    }
    tls
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_default_for_empty_or_whitespace() {
        assert!(matches!(parse_selector(""), NodeSelector::Default));
        assert!(matches!(parse_selector("   "), NodeSelector::Default));
    }

    #[test]
    fn selector_index_for_digits() {
        assert!(matches!(parse_selector("0"), NodeSelector::Index(0)));
        assert!(matches!(parse_selector("42"), NodeSelector::Index(42)));
    }

    #[test]
    fn selector_name_for_anything_else() {
        match parse_selector("香港") {
            NodeSelector::Name(s) => assert_eq!(s, "香港"),
            _ => panic!("expected Name"),
        }
        match parse_selector("hk-01") {
            NodeSelector::Name(s) => assert_eq!(s, "hk-01"),
            _ => panic!("expected Name"),
        }
    }

    fn fake_node(name: &str) -> ProxyNode {
        ProxyNode {
            name: name.into(),
            protocol: "vless",
            address: "example.com".into(),
            port: 443,
            outbound: json!({}),
        }
    }

    #[test]
    fn pick_default_returns_first() {
        let nodes = vec![fake_node("a"), fake_node("b")];
        let (i, n) = pick_node(&nodes, &NodeSelector::Default).unwrap();
        assert_eq!(i, 0);
        assert_eq!(n.name, "a");
    }

    #[test]
    fn pick_index_in_range() {
        let nodes = vec![fake_node("a"), fake_node("b"), fake_node("c")];
        let (i, n) = pick_node(&nodes, &NodeSelector::Index(2)).unwrap();
        assert_eq!(i, 2);
        assert_eq!(n.name, "c");
    }

    #[test]
    fn pick_index_out_of_range_errors() {
        let nodes = vec![fake_node("a")];
        assert!(pick_node(&nodes, &NodeSelector::Index(5)).is_err());
    }

    #[test]
    fn pick_name_case_insensitive_substring() {
        let nodes = vec![
            fake_node("US-01 美国"),
            fake_node("HK-02 香港高速"),
            fake_node("JP-03 日本"),
        ];
        let (i, _) = pick_node(&nodes, &NodeSelector::Name("香港".into())).unwrap();
        assert_eq!(i, 1);
        let (i, _) = pick_node(&nodes, &NodeSelector::Name("hk".into())).unwrap();
        assert_eq!(i, 1);
    }

    #[test]
    fn pick_name_no_match_errors() {
        let nodes = vec![fake_node("a")];
        assert!(pick_node(&nodes, &NodeSelector::Name("zzz".into())).is_err());
    }

    #[test]
    fn pick_from_empty_errors() {
        let nodes: Vec<ProxyNode> = vec![];
        assert!(pick_node(&nodes, &NodeSelector::Default).is_err());
    }

    #[test]
    fn parse_subscription_vless_reality() {
        let text = "vless://aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee@example.com:443\
            ?type=tcp&security=reality&pbk=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\
            &sid=abcd&sni=cdn.cloudflare.com&fp=chrome&flow=xtls-rprx-vision#HK-01";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "HK-01");
        assert_eq!(nodes[0].protocol, "vless");
        assert_eq!(nodes[0].port, 443);
        let stream = &nodes[0].outbound["streamSettings"];
        assert_eq!(stream["security"], "reality");
        assert!(stream["realitySettings"]["publicKey"].is_string());
    }

    #[test]
    fn parse_subscription_vless_tls() {
        let text = "vless://uuid-here@example.com:443?type=tcp&security=tls&sni=foo.com#TLS-01";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        let stream = &nodes[0].outbound["streamSettings"];
        assert_eq!(stream["security"], "tls");
        assert_eq!(stream["tlsSettings"]["serverName"], "foo.com");
    }

    #[test]
    fn parse_subscription_vless_ws_with_host_and_path() {
        let text = "vless://uuid@example.com:443\
            ?type=ws&security=tls&host=cdn.example.com&path=%2Fws%2Fendpoint&sni=cdn.example.com#WS-Node";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        let stream = &nodes[0].outbound["streamSettings"];
        assert_eq!(stream["network"], "ws");
        assert_eq!(stream["wsSettings"]["path"], "/ws/endpoint");
        assert_eq!(stream["wsSettings"]["headers"]["Host"], "cdn.example.com");
        assert_eq!(stream["security"], "tls");
        assert_eq!(stream["tlsSettings"]["serverName"], "cdn.example.com");
    }

    #[test]
    fn parse_subscription_vless_ws_path_only_no_host_header() {
        let text = "vless://uuid@example.com:443?type=ws&security=none&path=/api#WS-Bare";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        let stream = &nodes[0].outbound["streamSettings"];
        assert_eq!(stream["network"], "ws");
        assert_eq!(stream["wsSettings"]["path"], "/api");
        assert!(stream["wsSettings"].get("headers").is_none());
    }

    #[test]
    fn parse_subscription_vless_grpc_gun_mode() {
        let text = "vless://uuid@example.com:443\
            ?type=grpc&security=tls&serviceName=my-service&sni=example.com#GRPC-Gun";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        let stream = &nodes[0].outbound["streamSettings"];
        assert_eq!(stream["network"], "grpc");
        assert_eq!(stream["grpcSettings"]["serviceName"], "my-service");
        assert_eq!(stream["grpcSettings"]["multiMode"], false);
    }

    #[test]
    fn parse_subscription_vless_grpc_multi_mode() {
        let text = "vless://uuid@example.com:443\
            ?type=grpc&security=tls&serviceName=svc&mode=multi#GRPC-Multi";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        let stream = &nodes[0].outbound["streamSettings"];
        assert_eq!(stream["grpcSettings"]["multiMode"], true);
    }

    #[test]
    fn parse_subscription_vless_h2_alias_normalized_to_http() {
        let text = "vless://uuid@example.com:443\
            ?type=h2&security=tls&host=a.com,b.com&path=/p#H2";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        let stream = &nodes[0].outbound["streamSettings"];
        // h2 应当被归一化为 http（xray 的 network 字段名）
        assert_eq!(stream["network"], "http");
        assert_eq!(stream["httpSettings"]["path"], "/p");
        assert_eq!(stream["httpSettings"]["host"], serde_json::json!(["a.com", "b.com"]));
    }

    #[test]
    fn parse_subscription_skips_unsupported_transport_kcp() {
        let text = "vless://uuid@example.com:443?type=kcp&security=none#KCP";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 0);
    }

    #[test]
    fn parse_subscription_skips_unsupported_scheme() {
        let text = "hysteria2://pwd@example.com:443#H2\nsocks5://u:p@example.com:1080#X";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 0);
    }

    #[test]
    fn parse_subscription_mixed_and_blank_lines() {
        let text = "\n\
            vless://uuid@a.com:443?security=reality&pbk=xyz#A\n\
            \n\
            # comment that won't match\n\
            vless://uuid@b.com:443?security=none#B\n";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "A");
        assert_eq!(nodes[1].name, "B");
    }

    #[test]
    fn parse_subscription_trojan_default_tls() {
        let text = "trojan://my-password@example.com:443?sni=cdn.foo.com#TJ-01";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].protocol, "trojan");
        assert_eq!(nodes[0].name, "TJ-01");
        assert_eq!(nodes[0].outbound["settings"]["servers"][0]["password"], "my-password");
        let stream = &nodes[0].outbound["streamSettings"];
        assert_eq!(stream["security"], "tls");
        assert_eq!(stream["tlsSettings"]["serverName"], "cdn.foo.com");
    }

    #[test]
    fn parse_subscription_trojan_percent_decoded_password() {
        // password "p@ss:w/d" encoded as "p%40ss%3Aw%2Fd"
        let text = "trojan://p%40ss%3Aw%2Fd@example.com:443#TJ-02";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].outbound["settings"]["servers"][0]["password"], "p@ss:w/d");
    }

    #[test]
    fn parse_subscription_trojan_ws_parsed() {
        let text = "trojan://pwd@example.com:443\
            ?type=ws&security=tls&sni=foo.com&host=foo.com&path=/trojan#TJ-WS";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].protocol, "trojan");
        let stream = &nodes[0].outbound["streamSettings"];
        assert_eq!(stream["network"], "ws");
        assert_eq!(stream["wsSettings"]["path"], "/trojan");
        assert_eq!(stream["wsSettings"]["headers"]["Host"], "foo.com");
        assert_eq!(stream["security"], "tls");
    }

    #[test]
    fn parse_subscription_ss_sip002() {
        // base64("aes-256-gcm:my-pass") = "YWVzLTI1Ni1nY206bXktcGFzcw"
        let text = "ss://YWVzLTI1Ni1nY206bXktcGFzcw@example.com:8388#SS-01";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].protocol, "ss");
        let server = &nodes[0].outbound["settings"]["servers"][0];
        assert_eq!(server["method"], "aes-256-gcm");
        assert_eq!(server["password"], "my-pass");
        assert_eq!(server["port"], 8388);
    }

    #[test]
    fn parse_subscription_ss_url_safe_base64() {
        // SS userinfo using URL-safe base64 ("-" / "_") + missing padding
        // base64("chacha20-ietf-poly1305:abc?") standard = "Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTphYmM/"
        // URL-safe variant: replace "/" with "_" and drop padding
        let text = "ss://Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTphYmM_@example.com:8388#URLSafe";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 1);
        let server = &nodes[0].outbound["settings"]["servers"][0];
        assert_eq!(server["method"], "chacha20-ietf-poly1305");
        assert_eq!(server["password"], "abc?");
    }

    #[test]
    fn parse_subscription_ss_with_plugin_is_skipped() {
        let text = "ss://YWVzLTI1Ni1nY206cHc@example.com:8388?plugin=obfs-local#WithPlugin";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 0);
    }

    #[test]
    fn parse_subscription_mixed_protocols() {
        let vmess_json = r#"{"v":"2","ps":"VM","add":"vm.example.com","port":443,"id":"uuid-v","aid":0,"scy":"auto","net":"tcp","tls":"tls","sni":"vm.example.com"}"#;
        let text = format!(
            "vless://uuid@a.com:443?security=reality&pbk=xyz#V\n\
             trojan://pwd@b.com:443#T\n\
             ss://YWVzLTI1Ni1nY206cHc@c.com:8388#S\n\
             {}\n",
            vmess_uri(vmess_json)
        );
        let nodes = parse_subscription(&text);
        assert_eq!(nodes.len(), 4);
        assert_eq!(nodes[0].protocol, "vless");
        assert_eq!(nodes[1].protocol, "trojan");
        assert_eq!(nodes[2].protocol, "ss");
        assert_eq!(nodes[3].protocol, "vmess");
    }

    fn vmess_uri(json: &str) -> String {
        use base64::{engine::general_purpose, Engine as _};
        format!("vmess://{}", general_purpose::STANDARD.encode(json))
    }

    #[test]
    fn parse_subscription_vmess_tls() {
        let json = r#"{"v":"2","ps":"VM-01","add":"vm.example.com","port":443,"id":"uuid-here","aid":0,"scy":"auto","net":"tcp","tls":"tls","sni":"vm.example.com","alpn":"h2,http/1.1","fp":"chrome"}"#;
        let nodes = parse_subscription(&vmess_uri(json));
        assert_eq!(nodes.len(), 1);
        let n = &nodes[0];
        assert_eq!(n.protocol, "vmess");
        assert_eq!(n.name, "VM-01");
        assert_eq!(n.port, 443);
        let user = &n.outbound["settings"]["vnext"][0]["users"][0];
        assert_eq!(user["id"], "uuid-here");
        assert_eq!(user["alterId"], 0);
        assert_eq!(user["security"], "auto");
        let stream = &n.outbound["streamSettings"];
        assert_eq!(stream["security"], "tls");
        assert_eq!(stream["tlsSettings"]["serverName"], "vm.example.com");
        assert_eq!(stream["tlsSettings"]["alpn"], serde_json::json!(["h2", "http/1.1"]));
    }

    #[test]
    fn parse_subscription_vmess_none_security() {
        let json = r#"{"v":"2","ps":"VM-Plain","add":"vm.example.com","port":80,"id":"uuid","aid":0,"net":"tcp","tls":""}"#;
        let nodes = parse_subscription(&vmess_uri(json));
        assert_eq!(nodes.len(), 1);
        let stream = &nodes[0].outbound["streamSettings"];
        assert_eq!(stream["security"], "none");
        assert!(stream.get("tlsSettings").is_none());
    }

    #[test]
    fn parse_subscription_vmess_port_and_aid_as_strings() {
        let json = r#"{"v":"2","ps":"VM-Str","add":"vm.example.com","port":"8443","id":"uuid","aid":"2","net":"tcp","tls":"tls"}"#;
        let nodes = parse_subscription(&vmess_uri(json));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].port, 8443);
        let user = &nodes[0].outbound["settings"]["vnext"][0]["users"][0];
        assert_eq!(user["alterId"], 2);
    }

    #[test]
    fn parse_subscription_vmess_ws_parsed() {
        let json = r#"{"v":"2","ps":"VM-WS","add":"vm.example.com","port":443,"id":"uuid","aid":0,"net":"ws","tls":"tls","host":"cdn.vm.com","path":"/vm-ws"}"#;
        let nodes = parse_subscription(&vmess_uri(json));
        assert_eq!(nodes.len(), 1);
        let stream = &nodes[0].outbound["streamSettings"];
        assert_eq!(stream["network"], "ws");
        assert_eq!(stream["wsSettings"]["path"], "/vm-ws");
        assert_eq!(stream["wsSettings"]["headers"]["Host"], "cdn.vm.com");
        assert_eq!(stream["security"], "tls");
    }

    #[test]
    fn parse_subscription_vmess_fragment_overrides_ps() {
        let json = r#"{"v":"2","ps":"Inside","add":"vm.example.com","port":443,"id":"uuid","aid":0,"net":"tcp","tls":"tls"}"#;
        let line = format!("{}#Outside", vmess_uri(json));
        let nodes = parse_subscription(&line);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "Outside");
    }

    #[test]
    fn parse_subscription_vmess_invalid_base64() {
        let text = "vmess://!!!not-base64";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 0);
    }

    #[test]
    fn parse_subscription_vmess_valid_base64_bad_json() {
        // base64("not a json") = "bm90IGEganNvbg=="
        let text = "vmess://bm90IGEganNvbg==";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 0);
    }
}
