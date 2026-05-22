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

/// 公共：把 URI 的 type / security 字段翻译成 xray streamSettings。
/// 目前只支持 type=tcp；遇到 ws/grpc/h2/kcp/quic 等会 bail（被 parse_subscription 跳过）。
fn build_stream_settings(
    q: &HashMap<String, String>,
    address: &str,
    default_security: &str,
) -> Result<Value> {
    let network = q.get("type").map(String::as_str).unwrap_or("tcp");
    anyhow::ensure!(
        network == "tcp",
        "暂不支持 type={network} 的流传输（仅支持 tcp）"
    );

    let security_raw = q.get("security").map(String::as_str).unwrap_or("");
    let security = if security_raw.is_empty() {
        default_security
    } else {
        security_raw
    };

    let mut stream = json!({
        "network": "tcp",
        "security": security,
    });

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
    fn parse_subscription_skips_ws_transport() {
        let text = "vless://uuid@example.com:443?type=ws&security=tls#WS-Node";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 0);
    }

    #[test]
    fn parse_subscription_skips_unsupported_scheme() {
        let text = "vmess://eyJ2IjoiMiJ9\nhysteria2://pwd@example.com:443#H2";
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
    fn parse_subscription_trojan_skips_ws() {
        let text = "trojan://pwd@example.com:443?type=ws&security=tls&sni=foo.com#WS";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 0);
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
        let text = "\
            vless://uuid@a.com:443?security=reality&pbk=xyz#V\n\
            trojan://pwd@b.com:443#T\n\
            ss://YWVzLTI1Ni1nY206cHc@c.com:8388#S\n\
            vmess://eyJ2IjoiMiJ9#VM\n";
        let nodes = parse_subscription(text);
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].protocol, "vless");
        assert_eq!(nodes[1].protocol, "trojan");
        assert_eq!(nodes[2].protocol, "ss");
    }
}
