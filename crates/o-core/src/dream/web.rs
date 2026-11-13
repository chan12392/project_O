//! Web materials for `o dream add`: which addresses may be fetched, the fetch itself
//! (curl, redirects checked one by one), and the page turned into text.

use std::net::{IpAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use regex::Regex;

use super::one_line;
use crate::sys::proc;

/// What a web address may answer with before it is cut to `MAX_TEXT_BYTES` of text.
const MAX_FETCH_BYTES: usize = 5 * 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(25);

/// Why a web address is not fetched, if it is not: only public http(s) hosts, never a provider's server.
pub(super) fn refuse_address(address: &str) -> Option<String> {
    let rest = address.strip_prefix("https://").or_else(|| address.strip_prefix("http://"));
    let Some(rest) = rest else { return Some("http:// 또는 https:// 주소만 넣을 수 있습니다".into()) };
    // Braces are curl's own list syntax and a backslash is read as a slash by some parsers: one address, one request.
    if address.len() > 2000 || address.chars().any(|c| c.is_whitespace() || c.is_control() || "{}\\".contains(c)) { return Some("주소가 올바르지 않습니다".into()); }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') { return Some("로그인 정보가 든 주소는 넣을 수 없습니다".into()); }
    let bracketed = authority.starts_with('[');
    let host = if let Some(v6) = authority.strip_prefix('[') { v6.split(']').next().unwrap_or("") } else { authority.rsplit_once(':').map_or(authority, |(host, _)| host) }.to_lowercase();
    // The host is judged as written, so it is written plainly: the fetcher decodes "%31%32%37.0.0.1"
    // and folds full-width digits into 127.0.0.1, which the checks below would read as a name.
    // An international name goes in its punycode form (xn--).
    let plain = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-' || (bracketed && c == ':');
    let port = if bracketed { authority.split_once(']').map_or("", |(_, after)| after) } else { authority.rsplit_once(':').map_or("", |(_, port)| port) };
    let port = port.strip_prefix(':').unwrap_or(port);
    if !host.chars().all(plain) || !port.chars().all(|c| c.is_ascii_digit()) { return Some("주소의 호스트는 영문·숫자·점·하이픈으로만 적습니다(한글 도메인은 xn-- 표기)".into()); }
    // "localhost." is localhost.
    let host = host.trim_end_matches('.');
    let local = LOCAL;
    if let Ok(ip) = host.parse::<IpAddr>() {
        return inside(ip).then(|| local.into());
    }
    if bracketed { return Some("주소가 올바르지 않습니다".into()); }
    // Not four plain numbers, yet read as an address by the fetcher: 127.1, 0x7f.0.0.1, 2130706433.
    let numeric = host.split('.').all(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()) && part.chars().all(|c| c.is_ascii_hexdigit() || c == 'x'));
    if host.is_empty() || numeric || !host.contains('.') || host == "localhost" || [".localhost", ".local", ".internal"].iter().any(|end| host.ends_with(end)) { return Some(local.into()); }
    o_guard::rules::find_hits(address, o_guard::rules::code_rules()).first().map(|hit| hit.reason.to_string())
}

const LOCAL: &str = "이 PC나 내부 네트워크의 주소는 넣을 수 없습니다";

/// Whether an address is this PC or its network.
fn inside(ip: IpAddr) -> bool {
    let v4 = |ip: std::net::Ipv4Addr| {
        let [a, b, ..] = ip.octets();
        // Also "this network" (0/8) and carrier-grade NAT (100.64/10).
        a == 0 || (a == 100 && b & 0xc0 == 64) || ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_broadcast()
    };
    match ip {
        IpAddr::V4(ip) => v4(ip),
        IpAddr::V6(ip) => {
            let first = ip.segments()[0];
            // An IPv4 address written as IPv6 is judged as the IPv4 address it is.
            let mapped = ip.to_ipv4_mapped().or_else(|| ip.to_ipv4()).is_some_and(v4);
            mapped || ip.is_loopback() || ip.is_unspecified() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
        }
    }
}

/// The host and port of an address `refuse_address` let through.
fn host_port(address: &str) -> Option<(String, u16)> {
    let (rest, default) = match address.strip_prefix("https://") { Some(rest) => (rest, 443), None => (address.strip_prefix("http://")?, 80) };
    let authority = rest.split(['/', '?', '#']).next()?;
    let (host, port) = match authority.strip_prefix('[') {
        Some(v6) => { let (host, after) = v6.split_once(']')?; (host, after.strip_prefix(':')) }
        None => authority.rsplit_once(':').map_or((authority, None), |(host, port)| (host, Some(port))),
    };
    let port = match port.filter(|port| !port.is_empty()) { Some(port) => port.parse().ok()?, None => default };
    Some((host.to_string(), port))
}

/// A name is judged by where it leads, not only by how it is written: a public name may
/// answer with an address of this PC or its network. Every address it answers with must be
/// outside, and the fetch is tied to one of those (curl's `--resolve`), so a second lookup
/// cannot answer differently. `None` for an address written as numbers, already judged.
pub(super) fn pin(host: &str, port: u16, answers: &[IpAddr]) -> Result<Option<String>, String> {
    if host.parse::<IpAddr>().is_ok() { return Ok(None); }
    if answers.iter().any(|ip| inside(*ip)) { return Err(LOCAL.into()); }
    let ip = answers.first().ok_or("주소를 찾지 못했습니다")?;
    Ok(Some(match ip { IpAddr::V4(ip) => format!("{host}:{port}:{ip}"), IpAddr::V6(ip) => format!("{host}:{port}:[{ip}]") }))
}

fn resolve(host: &str, port: u16) -> Vec<IpAddr> {
    if host.parse::<IpAddr>().is_ok() { return Vec::new(); }
    (host, port).to_socket_addrs().map(|found| found.map(|address| address.ip()).collect()).unwrap_or_default()
}

/// The system's curl: Windows ships it beside the system tools, elsewhere it is on PATH.
fn curl() -> PathBuf {
    let system = std::env::var_os("SystemRoot").map(|root| PathBuf::from(root).join("System32").join("curl.exe"));
    system.filter(|path| path.is_file()).unwrap_or_else(|| PathBuf::from("curl"))
}

/// One hop's curl arguments. "-q" first: the person's own curl settings file (headers, a proxy
/// named there) stays out of it. `--noproxy "*"`: a proxy from the environment would look the
/// name up itself and connect where it likes, past the address that was checked, so the fetch
/// connects directly or fails.
pub(super) fn curl_args(pinned: Option<&str>, address: &str) -> Vec<String> {
    let mut args: Vec<String> = ["-q", "-sS", "--globoff", "--compressed", "--noproxy", "*", "--proto", "=http,https", "--max-time", "20"].map(String::from).to_vec();
    if let Some(pinned) = pinned { args.extend(["--resolve".to_string(), pinned.to_string()]); }
    args.extend(["--max-filesize", &MAX_FETCH_BYTES.to_string(), "-A", "project_O", "-w", FETCH_MARK, "--", address].map(String::from));
    args
}

pub(super) fn fetch(address: &str) -> Result<String, String> {
    // One hop per request and no curl of ours follows on its own: every address a page sends
    // on to is checked like the first, so a public page cannot lead to this PC or its network.
    let mut address = address.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let (host, port) = host_port(&address).ok_or("주소가 올바르지 않습니다")?;
        let pinned = pin(&host, port, &resolve(&host, port)).map_err(|reason| format!("주소를 읽지 못했습니다: {reason}"))?;
        let mut command = Command::new(curl());
        command.args(curl_args(pinned.as_deref(), &address));
        let (status, out, err) = proc::capture(&mut command, FETCH_TIMEOUT).map_err(|e| format!("주소를 읽지 못했습니다: {e}"))?;
        if !status.success() { return Err(format!("주소를 읽지 못했습니다: {}", one_line(&err))); }
        let (body, code, next) = fetched(&out).ok_or("주소를 읽지 못했습니다: 응답을 읽을 수 없습니다")?;
        match code {
            200..=299 => return Ok(body.to_string()),
            300..=399 if !next.is_empty() => {
                if let Some(reason) = refuse_address(next) { return Err(format!("이 주소가 넘겨 보내는 곳은 읽지 않습니다: {reason}")); }
                address = next.to_string();
            }
            _ => return Err(format!("주소를 읽지 못했습니다: 응답 코드 {code}")),
        }
    }
    Err("주소를 읽지 못했습니다: 넘겨 보내기가 너무 많습니다".into())
}

const MAX_REDIRECTS: usize = 5;
/// What curl writes after the body: the response code and, for a redirect, where it points (made absolute by curl).
const FETCH_MARK: &str = "\n\u{1e}o-dream %{http_code} %{redirect_url}";

/// A fetch's output as (body, response code, redirect target).
pub(super) fn fetched(out: &str) -> Option<(&str, u16, &str)> {
    let (body, mark) = out.rsplit_once("\n\u{1e}o-dream ")?;
    let (code, next) = mark.split_once(' ')?;
    Some((body, code.parse().ok()?, next.trim()))
}

fn entities(text: &str) -> String {
    let named = text.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'");
    let numeric = Regex::new(r"&#(x[0-9a-fA-F]{1,6}|[0-9]{1,7});").expect("pattern");
    let decoded = numeric.replace_all(&named, |found: &regex::Captures| {
        let code = &found[1];
        let value = code.strip_prefix('x').map_or_else(|| code.parse::<u32>().ok(), |hex| u32::from_str_radix(hex, 16).ok());
        value.and_then(char::from_u32).filter(|c| !c.is_control()).map_or_else(String::new, String::from)
    });
    decoded.replace("&amp;", "&")
}

/// A web page as plain text with its headings kept, and its `<title>`. Text that is not HTML is returned as is.
pub(super) fn page_text(raw: &str) -> (Option<String>, String) {
    let head = raw.chars().take(2000).collect::<String>().to_lowercase();
    if !(head.contains("<html") || head.contains("<!doctype html") || head.contains("<body") || head.contains("<head")) { return (None, raw.to_string()); }
    let pattern = |p: &str| Regex::new(p).expect("pattern");
    let title = pattern(r"(?is)<title[^>]*>(.*?)</title>").captures(raw).map(|c| one_line(&entities(&c[1]))).filter(|t| !t.is_empty());
    let text = pattern(r"(?is)<!--.*?-->|<(script|style|noscript|svg|head|template)\b.*?</(script|style|noscript|svg|head|template)\s*>").replace_all(raw, " ");
    let text = pattern(r"(?i)<h[1-6]\b[^>]*>").replace_all(&text, "\n\n## ");
    let text = pattern(r"(?i)<li\b[^>]*>").replace_all(&text, "\n- ");
    let text = pattern(r"(?i)<br\s*/?>|</(p|div|section|article|header|footer|h[1-6]|ul|ol|tr|table|blockquote|pre)\s*>").replace_all(&text, "\n\n");
    let text = pattern(r"(?s)<[^>]{0,2000}>").replace_all(&text, "");
    let text = entities(&text);
    let mut out = String::new();
    let mut blank = true;
    for line in text.lines().map(one_line) {
        if line.is_empty() || line == "-" || line == "##" {
            if !blank { out.push('\n'); }
            blank = true;
        } else {
            out.push_str(&line);
            out.push('\n');
            blank = false;
        }
    }
    (title, out.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_public_name_that_answers_with_an_inside_address_is_not_fetched() {
        let ip = |text: &str| text.parse::<IpAddr>().unwrap();
        assert_eq!(pin("example.org", 443, &[ip("93.184.215.14")]), Ok(Some("example.org:443:93.184.215.14".into())));
        assert_eq!(pin("example.org", 8080, &[ip("2606:2800:21f:cb07:6820:80da:af6b:8b2c")]), Ok(Some("example.org:8080:[2606:2800:21f:cb07:6820:80da:af6b:8b2c]".into())));
        for inside in ["127.0.0.1", "10.0.0.5", "192.168.0.1", "169.254.169.254", "100.64.0.1", "::1", "fd00::1", "::ffff:127.0.0.1"] {
            assert_eq!(pin("example.org", 443, &[ip("93.184.215.14"), ip(inside)]), Err(LOCAL.into()), "{inside}");
        }
        assert!(pin("example.org", 443, &[]).is_err());
        // Written as numbers: judged by `refuse_address`, nothing to look up.
        assert_eq!(pin("93.184.215.14", 80, &[]), Ok(None));
        // Every hop goes direct: a proxy from the environment would look the name up again.
        for pinned in [Some("example.org:80:93.184.215.14"), None] {
            let args = curl_args(pinned, "http://example.org/");
            assert_eq!(args[0], "-q");
            assert!(args.windows(2).any(|pair| pair == ["--noproxy", "*"]), "{args:?}");
            assert_eq!(args.windows(2).any(|pair| pair[0] == "--resolve" && Some(pair[1].as_str()) == pinned), pinned.is_some());
            assert_eq!(args[args.len() - 2..], ["--", "http://example.org/"]);
        }
        assert_eq!(host_port("https://example.org/a?b"), Some(("example.org".into(), 443)));
        assert_eq!(host_port("http://example.org:8080"), Some(("example.org".into(), 8080)));
        assert_eq!(host_port("http://[2606:2800::1]:81/x"), Some(("2606:2800::1".into(), 81)));
        assert_eq!(host_port("https://example.org:/x"), Some(("example.org".into(), 443)));
    }
}
