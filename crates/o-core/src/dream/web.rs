//! Web materials for `o dream add`: which addresses may be fetched, the fetch itself
//! (curl, redirects checked one by one), and the page turned into text.

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
    let local = "이 PC나 내부 네트워크의 주소는 넣을 수 없습니다";
    let inside = |ip: std::net::Ipv4Addr| {
        let [a, b, ..] = ip.octets();
        // Also "this network" (0/8) and carrier-grade NAT (100.64/10).
        a == 0 || (a == 100 && b & 0xc0 == 64) || ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_broadcast()
    };
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return inside(ip).then(|| local.into());
    }
    if let Ok(ip) = host.parse::<std::net::Ipv6Addr>() {
        let first = ip.segments()[0];
        // An IPv4 address written as IPv6 is judged as the IPv4 address it is.
        let mapped = ip.to_ipv4_mapped().or_else(|| ip.to_ipv4()).is_some_and(inside);
        return (mapped || ip.is_loopback() || ip.is_unspecified() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80).then(|| local.into());
    }
    if bracketed { return Some("주소가 올바르지 않습니다".into()); }
    // Not four plain numbers, yet read as an address by the fetcher: 127.1, 0x7f.0.0.1, 2130706433.
    let numeric = host.split('.').all(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()) && part.chars().all(|c| c.is_ascii_hexdigit() || c == 'x'));
    if host.is_empty() || numeric || !host.contains('.') || host == "localhost" || [".localhost", ".local", ".internal"].iter().any(|end| host.ends_with(end)) { return Some(local.into()); }
    o_guard::rules::find_hits(address, o_guard::rules::code_rules()).first().map(|hit| hit.reason.to_string())
}

/// The system's curl: Windows ships it beside the system tools, elsewhere it is on PATH.
fn curl() -> PathBuf {
    let system = std::env::var_os("SystemRoot").map(|root| PathBuf::from(root).join("System32").join("curl.exe"));
    system.filter(|path| path.is_file()).unwrap_or_else(|| PathBuf::from("curl"))
}

pub(super) fn fetch(address: &str) -> Result<String, String> {
    // One hop per request and no curl of ours follows on its own: every address a page sends
    // on to is checked like the first, so a public page cannot lead to this PC or its network.
    // "-q" first: the person's own curl settings (a proxy, headers) stay out of it.
    let mut address = address.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let mut command = Command::new(curl());
        command.args(["-q", "-sS", "--globoff", "--compressed", "--proto", "=http,https", "--max-time", "20"]);
        command.args(["--max-filesize", &MAX_FETCH_BYTES.to_string(), "-A", "project_O", "-w", FETCH_MARK, "--", &address]);
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
