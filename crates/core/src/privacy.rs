//! Tracking in received mail: tracking pixels, images and links that carry a
//! code for the recipient, click counters, and the platform that sent the
//! message. Heuristics only, and nothing here touches the network.
//!
//! The HTML read here is always the stored one, already serialized by ammonia
//! (see [`crate::reading::sanitize_html`]): tag and attribute names are lower
//! case, values are double-quoted with `&` and `"` escaped, and text never
//! contains a raw `<`. That regular shape is what lets [`tags`] read it
//! without a full parser.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageKind {
    /// Invisible, there only to report the opening: always removed.
    Pixel,
    /// Its address carries a code for the recipient.
    Personal,
    /// The same for everyone (logos, icons, photos).
    Generic,
}

/// Which remote images a rendered message may load. Tracking pixels never.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Images {
    #[default]
    Blocked,
    /// Only the generic ones (protection on).
    Safe,
    All,
}

// ------------------------------------------------------------------ tags

struct Attr<'a> {
    name: &'a str,
    value: String,
    /// Byte range of the whole `name="value"` in the document.
    span: (usize, usize),
}

struct Tag<'a> {
    name: &'a str,
    attrs: Vec<Attr<'a>>,
    /// Byte range of the whole tag, `<` to `>`.
    span: (usize, usize),
}

impl Tag<'_> {
    fn get(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .map(|a| a.value.as_str())
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{a0}")
        .replace("&amp;", "&")
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\u{a0}', "&nbsp;")
}

/// Start tags of the document, in order.
fn tags(html: &str) -> Vec<Tag<'_>> {
    let b = html.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = html[i..].find('<') {
        let start = i + off;
        let mut j = start + 1;
        while j < b.len() && b[j].is_ascii_alphanumeric() {
            j += 1;
        }
        if j == start + 1 {
            i = start + 1;
            continue;
        }
        let name = &html[start + 1..j];
        let mut attrs = Vec::new();
        loop {
            while j < b.len() && (b[j].is_ascii_whitespace() || b[j] == b'/') {
                j += 1;
            }
            if j >= b.len() || b[j] == b'>' {
                break;
            }
            let a_start = j;
            while j < b.len() && !b[j].is_ascii_whitespace() && !matches!(b[j], b'=' | b'>' | b'/')
            {
                j += 1;
            }
            let a_name = &html[a_start..j];
            let mut value = String::new();
            if j < b.len() && b[j] == b'=' {
                j += 1;
                if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                    let q = b[j];
                    let v_start = j + 1;
                    j = v_start;
                    while j < b.len() && b[j] != q {
                        j += 1;
                    }
                    value = unescape(&html[v_start..j.min(b.len())]);
                    j = (j + 1).min(b.len());
                } else {
                    let v_start = j;
                    while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                        j += 1;
                    }
                    value = unescape(&html[v_start..j]);
                }
            }
            if a_name.is_empty() {
                j += 1;
                continue;
            }
            attrs.push(Attr {
                name: a_name,
                value,
                span: (a_start, j),
            });
        }
        let end = (j + 1).min(b.len());
        out.push(Tag {
            name,
            attrs,
            span: (start, end),
        });
        i = end;
    }
    out
}

// ---------------------------------------------------------------- images

fn is_remote(src: &str) -> bool {
    let s = src.trim_start().to_ascii_lowercase();
    s.starts_with("http:") || s.starts_with("https:") || s.starts_with("//")
}

fn host_of(url: &str) -> String {
    let u = url.trim();
    let u = if u.starts_with("//") {
        format!("https:{u}")
    } else {
        u.to_string()
    };
    Url::parse(&u)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default()
}

/// Leading number of `"1"`, `"1px"`, `" 0 "`.
fn number(v: &str) -> Option<u32> {
    let d: String = v
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    d.parse().ok()
}

/// `width`/`height` from a style, and whether it hides the element.
fn style_box(style: &str) -> (Option<u32>, Option<u32>, bool) {
    let mut w = None;
    let mut h = None;
    let mut hidden = false;
    for decl in style.split(';') {
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim().to_ascii_lowercase();
        let v = v.trim_end_matches("!important").trim();
        match k.as_str() {
            "width" => w = number(v),
            "height" => h = number(v),
            "display" if v == "none" => hidden = true,
            "visibility" if v == "hidden" => hidden = true,
            "opacity"
                if v.trim_start_matches('0')
                    .trim_start_matches('.')
                    .trim_start_matches('0')
                    .is_empty() =>
            {
                hidden = true
            }
            "max-width" | "max-height" if number(v) == Some(0) => hidden = true,
            _ => {}
        }
    }
    (w, h, hidden)
}

/// Query parameters whose value usually names the recipient.
fn identity_param(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "u",
        "uid",
        "user",
        "userid",
        "user_id",
        "rcpt",
        "recipient",
        "email",
        "eid",
        "sid",
        "subscriber",
        "contact",
        "upn",
        "jwt",
        "hash",
        "token",
        "mc_eid",
        "mid",
    ]
    .contains(&n.as_str())
        || ["token", "email", "user", "subscriber", "recipient"]
            .iter()
            .any(|p| n.contains(p))
}

/// Raw `name=value` pairs of the query, with the name percent-decoded.
fn query_pairs(url: &Url) -> Vec<(String, String)> {
    url.query()
        .unwrap_or("")
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            let k = url::form_urlencoded::parse(k.as_bytes())
                .next()
                .map(|(k, _)| k.into_owned())
                .unwrap_or_else(|| k.to_string());
            let v = url::form_urlencoded::parse(format!("x={v}").as_bytes())
                .next()
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default();
            (k, v)
        })
        .collect()
}

fn base64(s: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = s.as_bytes();
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..=c.len() {
            out.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
        }
    }
    out
}

/// The address `me` appears in the URL, plain, percent-encoded or in base64.
fn names_me(url: &str, me: &str) -> bool {
    if me.is_empty() {
        return false;
    }
    let u = url.to_ascii_lowercase();
    let me = me.to_ascii_lowercase();
    if u.contains(&me) || u.contains(&me.replace('@', "%40")) {
        return true;
    }
    // base64 of the address, standard or URL-safe, padding dropped
    let b = base64(&me);
    let url_safe = b.replace('+', "-").replace('/', "_");
    url.contains(&b) || url.contains(&url_safe)
}

/// Addresses known to serve opening pixels, or a path that says so.
fn tracker_url(url: &str) -> bool {
    let full = if url.trim_start().starts_with("//") {
        format!("https:{}", url.trim())
    } else {
        url.trim().to_string()
    };
    let Ok(u) = Url::parse(&full) else {
        return false;
    };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let path = u.path().to_ascii_lowercase();
    if ["mailtrack.io", "sidekickopen", "pixel.", "open."]
        .iter()
        .any(|h| host.starts_with(h) || host.contains(&format!(".{h}")) || host == *h)
    {
        return true;
    }
    if [
        "/track/open",
        "/wf/open",
        "/e/o/",
        "/o.gif",
        "/open.aspx",
        "/beacon",
    ]
    .iter()
    .any(|p| path.contains(p))
    {
        return true;
    }
    path.split('/').any(|seg| {
        let stem = seg.split('.').next().unwrap_or("");
        [
            "open",
            "opened",
            "pixel",
            "beacon",
            "openrate",
            "trackopen",
            "track-open",
            "impression",
        ]
        .contains(&stem)
    })
}

/// Whether an image is a tracking pixel, carries a code for `me` or is the
/// same for everyone. Uncertain cases count as generic, so that a real image
/// is never hidden for good.
pub fn classify_image(
    src: &str,
    width: Option<&str>,
    height: Option<&str>,
    style: Option<&str>,
    me: &str,
) -> ImageKind {
    classify(src, width, height, style, me, &HashSet::new())
}

/// [`classify_image`], ignoring the query parameters in `varying`: those
/// whose value changes from one image of the message to the next name the
/// image, not the recipient (a code for the recipient is the same in all).
pub fn classify(
    src: &str,
    width: Option<&str>,
    height: Option<&str>,
    style: Option<&str>,
    me: &str,
    varying: &HashSet<String>,
) -> ImageKind {
    let (sw, sh, hidden) = style.map(style_box).unwrap_or((None, None, false));
    let w = width.and_then(number).or(sw);
    let h = height.and_then(number).or(sh);
    let tiny = matches!((w, h), (Some(w), Some(h)) if w <= 3 && h <= 3);
    let zero = w == Some(0) || h == Some(0);
    let large = w.is_some_and(|w| w > 3) || h.is_some_and(|h| h > 3);
    // a large image hidden by its style is a variant for other screens (shown
    // by a media query that sanitizing dropped), not a pixel; a tiny one with
    // a plain address (`spacer.gif`) is layout, the same for everyone
    let invisible = (hidden && !large) || tiny || zero;
    if (invisible && unique_address(src)) || (!large && tracker_url(src)) {
        return ImageKind::Pixel;
    }
    let personal_query = parse_src(src).is_some_and(|u| {
        query_pairs(&u).iter().any(|(k, v)| {
            identity_param(k) && v.len() >= 6 && !varying.contains(&k.to_ascii_lowercase())
        })
    });
    if personal_query || names_me(src, me) {
        ImageKind::Personal
    } else {
        ImageKind::Generic
    }
}

/// The address can tell one recipient or message from another: a query, a
/// long token in the path, or numeric codes in it.
fn unique_address(src: &str) -> bool {
    let Some(u) = parse_src(src) else {
        return true;
    };
    u.query().is_some_and(|q| !q.is_empty())
        || u.path().chars().filter(char::is_ascii_digit).count() >= 10
        || u.path().split('/').any(|seg| {
            let stem = seg.split('.').next().unwrap_or("");
            stem.len() >= 16
        })
}

fn parse_src(src: &str) -> Option<Url> {
    let s = src.trim();
    if s.starts_with("//") {
        Url::parse(&format!("https:{s}")).ok()
    } else {
        Url::parse(s).ok()
    }
}

/// Query parameters with a different value in different images of the document.
pub fn varying_params(html: &str) -> HashSet<String> {
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut varying = HashSet::new();
    for t in tags(html) {
        if !t.name.eq_ignore_ascii_case("img") {
            continue;
        }
        let Some(u) = t.get("src").and_then(parse_src) else {
            continue;
        };
        for (k, v) in query_pairs(&u) {
            let k = k.to_ascii_lowercase();
            match seen.get(&k) {
                Some(old) if *old != v => {
                    varying.insert(k);
                }
                Some(_) => {}
                None => {
                    seen.insert(k, v);
                }
            }
        }
    }
    varying
}

/// Every remote image of the document (and remote CSS backgrounds), with its
/// kind and host.
pub fn images(html: &str, me: &str) -> Vec<(ImageKind, String)> {
    let varying = varying_params(html);
    let mut out = Vec::new();
    for t in tags(html) {
        if t.name.eq_ignore_ascii_case("img") {
            if let Some(src) = t.get("src").filter(|s| is_remote(s)) {
                let kind = classify(
                    src,
                    t.get("width"),
                    t.get("height"),
                    t.get("style"),
                    me,
                    &varying,
                );
                out.push((kind, host_of(src)));
            }
        }
        if let Some(style) = t.get("style") {
            for u in css_urls(style) {
                if is_remote(&u) {
                    out.push((classify(&u, None, None, None, me, &varying), host_of(&u)));
                }
            }
        }
    }
    out
}

/// The `url(...)` values of a style, unquoted.
fn css_urls(style: &str) -> Vec<String> {
    css_url_spans(style)
        .into_iter()
        .map(|(_, _, u)| u)
        .collect()
}

/// (start, end, url) of each `url(...)` in a style.
fn css_url_spans(style: &str) -> Vec<(usize, usize, String)> {
    let lower = style.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = lower[i..].find("url(") {
        let start = i + off;
        let Some(close) = lower[start..].find(')') else {
            break;
        };
        let end = start + close + 1;
        let inner = style[start + 4..end - 1]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_string();
        out.push((start, end, inner));
        i = end;
    }
    out
}

/// The document with tracking pixels taken out and the remote images that
/// `images` does not allow stripped of their address. Also returns how many
/// remote images were held back (pixels apart) and how many pixels were
/// removed.
pub fn filter_html(html: &str, images: Images, me: &str) -> (String, usize, usize) {
    let varying = varying_params(html);
    let mut out = String::with_capacity(html.len());
    let mut last = 0;
    let mut blocked = 0;
    let mut pixels = 0;
    let allows = |kind: ImageKind| match images {
        Images::Blocked => false,
        Images::Safe => kind == ImageKind::Generic,
        Images::All => kind != ImageKind::Pixel,
    };
    for t in tags(html) {
        let is_img = t.name.eq_ignore_ascii_case("img");
        let src = if is_img { t.get("src") } else { None };
        // which attribute spans to cut, and a replacement style if any
        let mut cut: Vec<(usize, usize)> = Vec::new();
        let mut new_style: Option<(usize, usize, String)> = None;
        if let Some(src) = src {
            if src.trim_start().starts_with("data:") {
                // inline: loads nothing
            } else if is_remote(src) {
                let kind = classify(
                    src,
                    t.get("width"),
                    t.get("height"),
                    t.get("style"),
                    me,
                    &varying,
                );
                if kind == ImageKind::Pixel {
                    pixels += 1;
                    out.push_str(&html[last..t.span.0]);
                    last = t.span.1;
                    continue;
                }
                if !allows(kind) {
                    blocked += 1;
                    cut.extend(t.attrs.iter().filter(|a| a.name == "src").map(|a| a.span));
                }
            } else {
                // cid: and relative addresses cannot load anyway
                cut.extend(t.attrs.iter().filter(|a| a.name == "src").map(|a| a.span));
            }
        }
        if let Some(a) = t.attrs.iter().find(|a| a.name == "style") {
            let spans = css_url_spans(&a.value);
            if !spans.is_empty() {
                let mut style = String::new();
                let mut at = 0;
                for (s, e, u) in spans {
                    style.push_str(&a.value[at..s]);
                    let keep = u.to_ascii_lowercase().starts_with("data:")
                        || (is_remote(&u) && allows(classify(&u, None, None, None, me, &varying)));
                    if keep {
                        style.push_str(&a.value[s..e]);
                    } else {
                        style.push_str("none");
                    }
                    at = e;
                }
                style.push_str(&a.value[at..]);
                if style != a.value {
                    new_style = Some((a.span.0, a.span.1, style));
                }
            }
        }
        if cut.is_empty() && new_style.is_none() {
            continue;
        }
        // rebuild the tag with those attributes removed or replaced
        out.push_str(&html[last..t.span.0]);
        let mut at = t.span.0;
        let mut edits: Vec<(usize, usize, String)> = cut
            .into_iter()
            .map(|(s, e)| (s, e, String::new()))
            .collect();
        if let Some((s, e, style)) = new_style {
            edits.push((s, e, format!("style=\"{}\"", escape_attr(&style))));
        }
        edits.sort_by_key(|e| e.0);
        for (s, e, with) in edits {
            out.push_str(&html[at..s]);
            out.push_str(&with);
            at = e;
        }
        out.push_str(&html[at..t.span.1]);
        last = t.span.1;
    }
    out.push_str(&html[last..]);
    (out, blocked, pixels)
}

// ----------------------------------------------------------------- links

/// Parameters that only tell the sender who clicked and from where. On
/// LinkedIn also the recipient's code and one-time login token (`eid`,
/// `otpToken`): without them LinkedIn asks to sign in, if needed.
fn tracking_param(host: &str, name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if (host == "linkedin.com" || host.ends_with(".linkedin.com"))
        && ["eid", "otptoken", "refid"].contains(&n.as_str())
    {
        return true;
    }
    [
        "fbclid",
        "gclid",
        "dclid",
        "gbraid",
        "wbraid",
        "msclkid",
        "yclid",
        "twclid",
        "ttclid",
        "li_fat_id",
        "igshid",
        "mkt_tok",
        "_kx",
        "ck_subscriber_id",
        "lipi",
        "midtoken",
        "midsig",
        "trackingid",
        "mc_cid",
        "mc_eid",
        "_hsenc",
        "_hsmi",
        "hsctatracking",
        "oly_enc_id",
        "oly_anon_id",
        "vero_id",
        "vero_conv",
        "ml_subscriber",
        "ml_subscriber_hash",
        "sc_cid",
        "_bta_tid",
        "_bta_c",
        "ref_src",
    ]
    .contains(&n.as_str())
        || ["utm_", "trk", "pk_", "mtm_", "__hs"]
            .iter()
            .any(|p| n.starts_with(p))
}

/// Tracking parameters that name a campaign, the same for every recipient.
/// LinkedIn's `trkEmail` and `lipi` carry a code for the message, so they
/// are not among them.
fn campaign_param(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["utm_", "pk_", "mtm_"].iter().any(|p| n.starts_with(p))
        || ["trk", "trkcampaign", "mc_cid", "sc_cid", "ref_src"].contains(&n.as_str())
}

/// Links that need their codes to work: left exactly as they are.
fn delicate(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    let words = [
        "unsub",
        "optout",
        "opt-out",
        "confirm",
        "verify",
        "verification",
        "reset",
        "password",
        "activate",
        "login",
        "signin",
        "sign-in",
        "auth",
        "token",
        "magic",
    ];
    words.iter().any(|w| path.contains(w))
        || query_pairs(url).iter().any(|(k, _)| {
            ["token", "key", "code", "otp", "sig", "signature"]
                .contains(&k.to_ascii_lowercase().as_str())
        })
}

/// The destination of a redirect that carries it in the clear
/// (`?url=https://…`), if this is one.
fn unwrap_redirect(url: &Url) -> Option<String> {
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let path = url.path().to_ascii_lowercase();
    let named: &[&str] = if host.starts_with("www.google.") || host.starts_with("google.") {
        if path == "/url" {
            &["q", "url"]
        } else {
            &[]
        }
    } else if host.ends_with("safelinks.protection.outlook.com") {
        &["url"]
    } else if ["l.facebook.com", "lm.facebook.com", "l.instagram.com"].contains(&host.as_str()) {
        &["u"]
    } else if host.ends_with("youtube.com") && path == "/redirect" {
        &["q"]
    } else {
        &[
            "url",
            "redirect",
            "redirect_url",
            "target",
            "dest",
            "destination",
            "link",
        ]
    };
    query_pairs(url)
        .into_iter()
        .find(|(k, v)| {
            named.contains(&k.to_ascii_lowercase().as_str())
                && (v.starts_with("https://") || v.starts_with("http://"))
        })
        .map(|(_, v)| v)
}

/// Opaque click counters: where the link leads only their server knows.
pub fn is_counter(url: &str) -> bool {
    let Ok(u) = Url::parse(url.trim()) else {
        return false;
    };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let path = u.path().to_ascii_lowercase();
    let first = host.split('.').next().unwrap_or("");
    let labels = host.split('.').count();
    let hosts = [
        "list-manage.com",
        "sendgrid.net",
        "mandrillapp.com",
        "hubspotlinks.com",
        "lnkd.in",
        "t.co",
        "bit.ly",
        "ow.ly",
        "tinyurl.com",
        "rebrand.ly",
        "mailtrack.io",
        "exct.net",
        "klclick.com",
        "klclick1.com",
        "awstrack.me",
        "mailchi.mp",
        "mlsend.com",
        "createsend1.com",
        "cmail19.com",
        "cmail20.com",
        "rs6.net",
        "mjt.lu",
        "sendibt3.com",
        "substack.com",
    ];
    (labels >= 3
        && [
            "click", "clicks", "links", "link", "trk", "track", "tracking", "email", "e", "em",
            "ct", "go", "ablink",
        ]
        .contains(&first))
        || (labels >= 3
            && first.starts_with("url")
            && first[3..].chars().all(|c| c.is_ascii_digit())
            && first.len() > 3)
        || hosts
            .iter()
            .any(|h| host == *h || host.ends_with(&format!(".{h}")))
            && !(host.ends_with("substack.com") && !path.starts_with("/redirect"))
        || [
            "/track/click",
            "/ls/click",
            "/wf/click",
            "/ss/c/",
            "/redirect/",
        ]
        .iter()
        .any(|p| path.contains(p))
}

/// What cleaning does to a link.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cleaned {
    pub url: String,
    /// Tracking parameters taken out.
    pub removed: Vec<String>,
    /// It went through a redirect whose destination was in the clear.
    pub unwrapped: bool,
}

/// The link without its tracking: known tracking parameters removed and
/// redirects that carry their destination opened up. Links that need their
/// codes (unsubscribe, confirm, login…) and anything unknown stay as they are.
pub fn clean(url: &str) -> Cleaned {
    let mut current = url.trim().to_string();
    let mut unwrapped = false;
    let mut removed = Vec::new();
    for _ in 0..3 {
        let Ok(u) = Url::parse(&current) else { break };
        if !matches!(u.scheme(), "http" | "https") || delicate(&u) {
            break;
        }
        if let Some(dest) = unwrap_redirect(&u) {
            current = dest;
            unwrapped = true;
            continue;
        }
        let pairs: Vec<String> = u
            .query()
            .unwrap_or("")
            .split('&')
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect();
        let named = query_pairs(&u);
        let host = u.host_str().unwrap_or("").to_ascii_lowercase();
        let mut kept = Vec::new();
        for (raw, (name, _)) in pairs.iter().zip(named.iter()) {
            if tracking_param(&host, name) {
                removed.push(name.clone());
            } else {
                kept.push(raw.as_str());
            }
        }
        if kept.len() != pairs.len() {
            let query = kept.join("&");
            let mut u = u;
            u.set_query(if kept.is_empty() { None } else { Some(&query) });
            current = u.to_string();
        }
        break;
    }
    Cleaned {
        url: if removed.is_empty() && !unwrapped {
            url.to_string()
        } else {
            current
        },
        removed,
        unwrapped,
    }
}

/// `http(s)://` addresses in plain text, cut where the reading view cuts them
/// (the same rule as its link regex).
pub fn text_urls(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let Some(off) = rest.find("http") else { break };
        let start = i + off;
        let s = &text[start..];
        if !(s.starts_with("http://") || s.starts_with("https://")) {
            i = start + 4;
            continue;
        }
        let len = s
            .find(|c: char| c.is_whitespace() || "<>\"'()]".contains(c))
            .unwrap_or(s.len());
        let url = s[..len].trim_end_matches(|c: char| ".,;:!?".contains(c));
        if url.len() > "https://".len() {
            out.push(url);
        }
        i = start + len.max(4);
    }
    out
}

/// The links of a message: the anchors of its HTML, or the addresses in its
/// text when it has no HTML.
pub fn links<'a>(html: Option<&'a str>, text: &'a str) -> Vec<String> {
    match html {
        Some(h) => tags(h)
            .into_iter()
            .filter(|t| t.name.eq_ignore_ascii_case("a"))
            .filter_map(|t| t.get("href").map(str::to_string))
            .filter(|h| h.starts_with("http://") || h.starts_with("https://"))
            .collect(),
        None => text_urls(text).into_iter().map(str::to_string).collect(),
    }
}

/// Clean versions of the addresses in a text, for those that change.
pub fn clean_text_links(text: &str) -> BTreeMap<String, String> {
    text_urls(text)
        .into_iter()
        .filter_map(|u| {
            let c = clean(u);
            (c.url != u).then(|| (u.to_string(), c.url))
        })
        .collect()
}

// --------------------------------------------------------------- headers

/// Header block of a raw message, up to the empty line.
pub fn header_block(raw: &[u8]) -> String {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n"))
        .unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

/// (lowercase name, unfolded value) of each header.
fn header_fields(block: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in block.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some((k, v)) = line.split_once(':') {
            out.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    out
}

/// Last two labels of a domain: `mail.linkedin.com` → `linkedin.com`.
fn base_domain(d: &str) -> String {
    let labels: Vec<&str> = d.trim().trim_end_matches('.').split('.').collect();
    labels[labels.len().saturating_sub(2)..]
        .join(".")
        .to_ascii_lowercase()
}

/// Who delivered the message for the sender, read from its headers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SentVia {
    /// "SendGrid", "linkedin.com (its own servers)"…
    pub via: String,
    /// Mass mailing: it has an unsubscribe header or `Precedence: bulk`.
    pub bulk: bool,
}

pub fn sent_via(headers: &str, from_addr: &str) -> SentVia {
    let fields = header_fields(headers);
    let bulk = fields.iter().any(|(k, v)| {
        k == "list-unsubscribe"
            || (k == "precedence" && ["bulk", "list"].contains(&v.to_ascii_lowercase().trim()))
    });
    // only headers about delivery, so a subject about Mailchimp is not one
    let delivery: String = fields
        .iter()
        .filter(|(k, _)| {
            k.starts_with("x-")
                || [
                    "received",
                    "return-path",
                    "dkim-signature",
                    "feedback-id",
                    "list-unsubscribe",
                    "message-id",
                    "list-id",
                    "sender",
                ]
                .contains(&k.as_str())
        })
        .map(|(k, v)| format!("{k}: {v}\n"))
        .collect::<String>()
        .to_ascii_lowercase();
    let platforms: &[(&[&str], &str)] = &[
        (&["mandrillapp"], "Mandrill (Mailchimp)"),
        (
            &["x-mc-user", "mcsv.net", "mailchimp", "list-manage"],
            "Mailchimp",
        ),
        (&["x-sg-", "sendgrid"], "SendGrid"),
        (&["x-mailgun", "mailgun"], "Mailgun"),
        (&["x-ses-outgoing", "amazonses.com"], "Amazon SES"),
        (&["x-hs-", "hubspot"], "HubSpot"),
        (&["sparkpost"], "SparkPost"),
        (&["x-pm-message-id", "postmarkapp"], "Postmark"),
        (&["sendinblue", "brevo"], "Brevo"),
        (
            &["exacttarget", "x-sfmc", "exct.net"],
            "Salesforce Marketing Cloud",
        ),
        (&["klaviyo"], "Klaviyo"),
        (&["ccsend.com", "constantcontact"], "Constant Contact"),
        (&["createsend", "cmail19", "cmail20"], "Campaign Monitor"),
        (&["mlsend", "mailerlite"], "MailerLite"),
        (&["convertkit", "kit-mail"], "Kit"),
        (&["mailjet", "mjt.lu"], "Mailjet"),
        (&["substack"], "Substack"),
        (&["beehiiv"], "beehiiv"),
        (&["customeriomail", "customer.io"], "Customer.io"),
        (&["zcsend", "zohocampaigns"], "Zoho Campaigns"),
    ];
    let platform = platforms
        .iter()
        .find(|(needles, _)| needles.iter().any(|n| delivery.contains(n)))
        .map(|(_, name)| (*name).to_string());
    let dkim: Vec<String> = fields
        .iter()
        .filter(|(k, _)| k == "dkim-signature")
        .filter_map(|(_, v)| {
            v.split(';')
                .filter_map(|p| p.trim().strip_prefix("d="))
                .map(|d| d.trim().to_ascii_lowercase())
                .next()
        })
        .collect();
    let from_domain = from_addr
        .rsplit('@')
        .next()
        .map(base_domain)
        .unwrap_or_default();
    let via = if let Some(p) = platform {
        p
    } else if dkim.iter().any(|d| base_domain(d) == "gmail.com") {
        "Gmail".into()
    } else if dkim
        .iter()
        .any(|d| ["outlook.com", "hotmail.com", "live.com"].contains(&base_domain(d).as_str()))
    {
        "Outlook.com".into()
    } else if !from_domain.is_empty() && dkim.iter().any(|d| base_domain(d) == from_domain) {
        format!("{from_domain} (its own servers)")
    } else if let Some(d) = dkim.first() {
        d.clone()
    } else {
        "unknown".into()
    };
    SentVia { via, bulk }
}

// ---------------------------------------------------------------- report

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostCount {
    pub host: String,
    pub count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LinkSummary {
    pub total: usize,
    /// With a code for the reader or the click (taken out when protection is on).
    pub personal: usize,
    /// With campaign tags only (`utm_*`…): they say where the click came
    /// from, not who made it.
    pub campaign: usize,
    /// Through a redirect whose destination is in the clear.
    pub redirects: usize,
    /// Through an opaque click counter.
    pub counters: usize,
    /// Names of the personal parameters found.
    pub params: Vec<String>,
    /// Names of the campaign tags found.
    pub campaign_params: Vec<String>,
    pub counter_hosts: Vec<HostCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub pixels: Vec<HostCount>,
    pub personal_images: Vec<HostCount>,
    pub generic_images: Vec<HostCount>,
    pub links: LinkSummary,
    /// `None` when the headers could not be read.
    pub sent_via: Option<SentVia>,
}

fn counts(items: impl Iterator<Item = String>) -> Vec<HostCount> {
    let mut m: BTreeMap<String, usize> = BTreeMap::new();
    for h in items {
        *m.entry(h).or_default() += 1;
    }
    let mut v: Vec<HostCount> = m
        .into_iter()
        .map(|(host, count)| HostCount { host, count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.host.cmp(&b.host)));
    v
}

pub fn report(
    html: Option<&str>,
    text: &str,
    headers: Option<&str>,
    from_addr: &str,
    me: &str,
) -> Report {
    let imgs = html.map(|h| images(h, me)).unwrap_or_default();
    let of = |kind: ImageKind| {
        counts(
            imgs.iter()
                .filter(|(k, _)| *k == kind)
                .map(|(_, h)| h.clone()),
        )
    };
    let mut links_sum = LinkSummary::default();
    let mut params: Vec<String> = Vec::new();
    let mut counter_hosts = Vec::new();
    for l in links(html, text) {
        links_sum.total += 1;
        let c = clean(&l);
        if c.unwrapped {
            links_sum.redirects += 1;
        }
        let (camp, pers): (Vec<String>, Vec<String>) =
            c.removed.into_iter().partition(|p| campaign_param(p));
        if !pers.is_empty() {
            links_sum.personal += 1;
        } else if !camp.is_empty() {
            links_sum.campaign += 1;
        }
        for p in pers {
            if !params.contains(&p) {
                params.push(p);
            }
        }
        for p in camp {
            if !links_sum.campaign_params.contains(&p) {
                links_sum.campaign_params.push(p);
            }
        }
        if !c.unwrapped && is_counter(&c.url) {
            links_sum.counters += 1;
            counter_hosts.push(host_of(&c.url));
        }
    }
    links_sum.params = params;
    links_sum.counter_hosts = counts(counter_hosts.into_iter());
    Report {
        pixels: of(ImageKind::Pixel),
        personal_images: of(ImageKind::Personal),
        generic_images: of(ImageKind::Generic),
        links: links_sum,
        sent_via: headers.map(|h| sent_via(h, from_addr)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "me@example.org";

    #[test]
    fn pixels_by_size_style_or_address() {
        let k = |src, w, h, s| classify_image(src, w, h, s, ME);
        let pixel = ImageKind::Pixel;
        assert_eq!(
            k("https://t.example/o?u=8f3a91c2", Some("1"), Some("1"), None),
            pixel
        );
        assert_eq!(
            k("https://t.example/a.gif?id=1", Some("0"), None, None),
            pixel
        );
        let hidden = "https://t.example/mk/op/Qw3rTy8uIo9pAs7dFg6h/qcgh";
        assert_eq!(k(hidden, None, None, Some("display:none")), pixel);
        // numeric codes in the path
        let numeric = "https://t.example/ab12cd/41234567/10123456/1234567890.gif";
        assert_eq!(k(numeric, Some("1"), Some("1"), None), pixel);
        assert_eq!(
            k("https://x.sendgrid.net/wf/open?upn=abc", None, None, None),
            pixel
        );
        assert_eq!(k("https://x.example/open.gif", None, None, None), pixel);
        // 3×1 with a code
        assert_eq!(
            k(
                "https://x.example/s.gif?mid=abc123",
                Some("3"),
                Some("1"),
                None
            ),
            pixel
        );
        // invisible but with a plain address: a layout spacer, the same for everyone
        let spacer = "https://www.gstatic.com/gumdrop/files/spacer-11.gif";
        assert_eq!(k(spacer, Some("1"), Some("3"), None), ImageKind::Generic);
        // an image named "open" but drawn large is a real image
        let open = "https://x.example/open.png";
        assert_eq!(k(open, Some("120"), Some("40"), None), ImageKind::Generic);
        // a spacer as wide as the layout is not a pixel
        let wide = "https://x.example/s.gif";
        assert_eq!(k(wide, Some("600"), Some("1"), None), ImageKind::Generic);
        // a large image hidden for this screen size is a variant, not a pixel
        let variant = "https://x.example/photo.jpg";
        let style = Some("display: none; height: 252px");
        assert_eq!(
            k(variant, Some("448"), Some("252"), style),
            ImageKind::Generic
        );
    }

    #[test]
    fn personal_and_generic_images() {
        let k = |src: &str| classify_image(src, Some("64"), Some("64"), None, ME);
        assert_eq!(
            k("https://x.example/i.png?uid=8f3a91c2"),
            ImageKind::Personal
        );
        assert_eq!(
            k("https://x.example/i.png?e=me%40example.org"),
            ImageKind::Personal
        );
        assert_eq!(
            k(&format!("https://x.example/{}", base64(ME))),
            ImageKind::Personal
        );
        // LinkedIn photos: signed CDN addresses, the same for every recipient
        assert_eq!(
            k("https://media.licdn.com/dms/image/v2/X1Y2Z3/profile-displayphoto-scale_400_400/0/1700?e=2147483647&v=beta&t=Qm9ndXNTaWduYXR1cmVGb3JFdmVyeW9uZTEy"),
            ImageKind::Generic
        );
        assert_eq!(
            k("https://static.licdn.com/aero-v1/sc/h/abc123def456ghi789jkl0mno"),
            ImageKind::Generic
        );
    }

    #[test]
    fn a_parameter_that_changes_between_images_names_the_image() {
        // newsletter images whose ?u= is each image's own id
        let html = "<img src=\"https://img.example/a?u=Ab12Cd34Ef56Gh78\" width=\"600\">\
<img src=\"https://img.example/a?u=Ab12Cd34Ef56Zz90\" width=\"300\">";
        assert!(images(html, ME)
            .iter()
            .all(|(k, _)| *k == ImageKind::Generic));
        assert_eq!(filter_html(html, Images::Safe, ME).1, 0);
        // the same code in every image is the recipient's
        let html = "<img src=\"https://img.example/a.png?uid=8f3a91c2\" width=\"600\">\
<img src=\"https://img.example/b.png?uid=8f3a91c2\" width=\"300\">";
        assert!(images(html, ME)
            .iter()
            .all(|(k, _)| *k == ImageKind::Personal));
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64("me@example.org"), "bWVAZXhhbXBsZS5vcmc");
        assert_eq!(base64("ab"), "YWI");
    }

    #[test]
    fn filter_removes_pixels_and_holds_back_images() {
        let html = "<p>hi</p><img src=\"https://t.example/p.gif?r=91c2\" width=\"1\" height=\"1\">\
<img src=\"https://cdn.example/logo.png\" alt=\"logo\" width=\"100\">\
<img src=\"https://cdn.example/me.png?uid=12345678\" width=\"50\">\
<div style=\"background:url(https://cdn.example/bg.png)\">x</div>";
        let (doc, blocked, pixels) = filter_html(html, Images::Blocked, ME);
        assert_eq!((blocked, pixels), (2, 1));
        assert!(!doc.contains("t.example") && !doc.contains("logo.png") && !doc.contains("me.png"));
        assert!(doc.contains("alt=\"logo\""));
        let (doc, blocked, pixels) = filter_html(html, Images::Safe, ME);
        assert_eq!((blocked, pixels), (1, 1));
        assert!(doc.contains("logo.png") && doc.contains("bg.png") && !doc.contains("me.png"));
        let (doc, blocked, pixels) = filter_html(html, Images::All, ME);
        assert_eq!((blocked, pixels), (0, 1));
        assert!(doc.contains("me.png") && !doc.contains("t.example"));
    }

    #[test]
    fn filter_keeps_escaping_intact() {
        let html = "<img src=\"https://cdn.example/a.png?x=1&amp;y=2\" alt=\"a &quot;b&quot;\" width=\"10\">";
        let (doc, _, _) = filter_html(html, Images::All, ME);
        assert_eq!(doc, html);
        let (doc, _, _) = filter_html(html, Images::Blocked, ME);
        assert_eq!(doc, "<img  alt=\"a &quot;b&quot;\" width=\"10\">");
    }

    #[test]
    fn cleans_tracking_parameters() {
        let c = clean("https://www.linkedin.com/comm/mynetwork/?lipi=urn%3Ali&midToken=AbC&midSig=dEf&trk=eml-x&trkEmail=y&eid=1a2b3c&otpToken=ZmFrZQ&page=2");
        assert_eq!(c.url, "https://www.linkedin.com/comm/mynetwork/?page=2");
        assert_eq!(
            c.removed,
            ["lipi", "midToken", "midSig", "trk", "trkEmail", "eid", "otpToken"]
        );
        // eid is somebody else's business elsewhere
        assert_eq!(
            clean("https://events.example/?eid=42").url,
            "https://events.example/?eid=42"
        );
        let c = clean("https://shop.example/p?utm_source=nl&utm_medium=email&ref=friend&coupon=X");
        assert_eq!(c.url, "https://shop.example/p?ref=friend&coupon=X");
        // nothing to remove: the very same string
        assert_eq!(clean("https://example.com").url, "https://example.com");
        assert_eq!(
            clean("mailto:a@b.it?utm_source=x").url,
            "mailto:a@b.it?utm_source=x"
        );
    }

    #[test]
    fn delicate_links_stay() {
        for u in [
            "https://x.example/unsubscribe?utm_source=nl&u=1",
            "https://www.linkedin.com/comm/mypreferences/u/emailunsub?action=x&eid=1&trk=y",
            "https://x.example/account/confirm?utm_source=nl",
            "https://x.example/go?token=abc&utm_source=nl",
        ] {
            assert_eq!(clean(u).url, u);
        }
    }

    #[test]
    fn opens_clear_redirects() {
        let c = clean("https://www.google.com/url?q=https://example.com/a?utm_source=x&sa=D");
        assert!(c.unwrapped);
        assert_eq!(c.url, "https://example.com/a");
        let c = clean("https://eur01.safelinks.protection.outlook.com/?url=https%3A%2F%2Fexample.com%2Fb&data=x");
        assert_eq!(c.url, "https://example.com/b");
    }

    #[test]
    fn counters() {
        assert!(is_counter("https://click.mail.example.com/abc"));
        assert!(is_counter(
            "https://us1.list-manage.com/track/click?u=1&id=2"
        ));
        assert!(is_counter("https://url1234.example.com/ls/click?upn=x"));
        assert!(is_counter("https://lnkd.in/abc"));
        assert!(!is_counter("https://www.linkedin.com/comm/feed/"));
        assert!(!is_counter("https://example.com/"));
    }

    #[test]
    fn urls_in_text() {
        let t = "See https://a.example/x?y=1. And (https://b.example/z) or http://c.example.";
        assert_eq!(
            text_urls(t),
            [
                "https://a.example/x?y=1",
                "https://b.example/z",
                "http://c.example"
            ]
        );
        let m = clean_text_links("go https://a.example/?utm_source=x now");
        assert_eq!(
            m.get("https://a.example/?utm_source=x").map(String::as_str),
            Some("https://a.example/")
        );
    }

    #[test]
    fn sender_platform() {
        let h = "Received: from o1.ptr.sendgrid.net (o1.ptr.sendgrid.net [1.2.3.4])\r\n\tby mx.google.com\r\nX-SG-EID: abc\r\nList-Unsubscribe: <mailto:u@x>\r\nSubject: mailchimp tips\r\n";
        assert_eq!(
            sent_via(h, "news@shop.example"),
            SentVia {
                via: "SendGrid".into(),
                bulk: true
            }
        );
        let h = "DKIM-Signature: v=1; a=rsa-sha256; d=linkedin.com; s=a\r\nSubject: Mailchimp\r\n";
        assert_eq!(
            sent_via(h, "messages-noreply@linkedin.com").via,
            "linkedin.com (its own servers)"
        );
        let h = "DKIM-Signature: v=1; d=gmail.com; s=20230601\r\n";
        assert_eq!(sent_via(h, "someone@gmail.com").via, "Gmail");
        assert_eq!(sent_via("", "a@b.it").via, "unknown");
    }

    #[test]
    fn header_block_stops_at_body() {
        assert_eq!(header_block(b"A: 1\r\nB: 2\r\n\r\nbody"), "A: 1\r\nB: 2");
        assert_eq!(header_block(b"A: 1\n\nbody"), "A: 1");
    }

    #[test]
    fn report_counts() {
        let html = "<img src=\"https://t.example/p.gif?r=91c2\" width=\"1\" height=\"1\">\
<img src=\"https://cdn.example/logo.png\" width=\"100\">\
<a href=\"https://shop.example/?utm_source=x\">a</a><a href=\"https://click.shop.example/q\">b</a>\
<a href=\"https://shop.example/p?mc_eid=91c2&amp;utm_medium=email\">d</a><a href=\"mailto:x@y\">c</a>";
        let r = report(Some(html), "", None, "n@shop.example", ME);
        assert_eq!(
            r.pixels,
            [HostCount {
                host: "t.example".into(),
                count: 1
            }]
        );
        assert_eq!(r.generic_images.len(), 1);
        let l = &r.links;
        assert_eq!((l.total, l.personal, l.campaign, l.counters), (3, 1, 1, 1));
        assert_eq!(l.params, ["mc_eid"]);
        assert_eq!(l.campaign_params, ["utm_source", "utm_medium"]);
        assert!(r.sent_via.is_none());
    }
}
