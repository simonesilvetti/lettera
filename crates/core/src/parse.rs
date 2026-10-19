//! RFC 5322 / MIME parsing on top of `mail-parser`.

use mail_parser::{Address, HeaderValue, Message, MessageParser, MimeHeaders};

use crate::model::Addr;
use crate::reading;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Envelope {
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub from: Addr,
    pub reply_to: Vec<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub date: i64,
    /// Guess from the top-level content type; corrected once the body arrives.
    pub has_attachments: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentMeta {
    /// Position in `Message::attachments()`, used to extract it later.
    pub index: u32,
    pub filename: String,
    pub mime: String,
    pub size: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedBody {
    /// Reading-mode text: text/plain, or the sanitized HTML converted to text.
    pub text: String,
    /// Sanitized HTML, if the message had an HTML part.
    pub html: Option<String>,
    pub attachments: Vec<AttachmentMeta>,
    pub snippet: String,
}

fn addrs(a: Option<&Address>) -> Vec<Addr> {
    a.map(|a| {
        a.iter()
            .filter_map(|x| {
                let addr = x.address()?.trim().to_string();
                if addr.is_empty() {
                    return None;
                }
                Some(Addr {
                    name: x.name().unwrap_or("").trim().to_string(),
                    addr,
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

fn id_list(v: &HeaderValue) -> Vec<String> {
    let raw: Vec<String> = match v {
        HeaderValue::Text(t) => vec![t.to_string()],
        HeaderValue::TextList(l) => l.iter().map(|t| t.to_string()).collect(),
        _ => Vec::new(),
    };
    raw.into_iter()
        .flat_map(|s| {
            s.split_whitespace()
                .map(clean_id)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A Message-ID without `<>`, whitespace or control characters: it ends up
/// in our own outgoing `In-Reply-To`/`References`, so nothing in it may be
/// able to start a new header line.
pub fn clean_id(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !c.is_control() && *c != '<' && *c != '>')
        .collect()
}

/// Small stable hash (FNV-1a) used to synthesize ids for messages without Message-ID.
pub fn fnv(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn envelope_of(m: &Message) -> Envelope {
    let from = addrs(m.from()).into_iter().next().unwrap_or_default();
    let subject = m.subject().unwrap_or("").trim().to_string();
    let date = m.date().map(|d| d.to_timestamp()).unwrap_or(0);
    let message_id = match m.message_id() {
        Some(id) if !clean_id(id).is_empty() => clean_id(id),
        _ => format!(
            "lettera-{:016x}@local",
            fnv(format!("{}|{}|{}", date, from.addr, subject).as_bytes())
        ),
    };
    let mut references = id_list(m.references());
    references.retain(|r| r != &message_id);
    let in_reply_to = id_list(m.in_reply_to()).into_iter().next();
    let has_attachments = m
        .root_part()
        .content_type()
        .map(|ct| {
            ct.ctype().eq_ignore_ascii_case("multipart")
                && ct
                    .subtype()
                    .map(|s| s.eq_ignore_ascii_case("mixed"))
                    .unwrap_or(false)
        })
        .unwrap_or(false);
    Envelope {
        message_id,
        in_reply_to,
        references,
        from,
        reply_to: addrs(m.reply_to()),
        to: addrs(m.to()),
        cc: addrs(m.cc()),
        subject,
        date,
        has_attachments,
    }
}

/// Parses a header block (or a whole message) into an envelope.
pub fn parse_envelope(raw: &[u8]) -> Envelope {
    match MessageParser::default().parse_headers(raw) {
        Some(m) => envelope_of(&m),
        None => Envelope {
            message_id: format!("lettera-{:016x}@local", fnv(raw)),
            ..Default::default()
        },
    }
}

fn attachment_list(m: &Message) -> Vec<AttachmentMeta> {
    let mut out = Vec::new();
    for (index, part) in m.attachments().enumerate() {
        let disposition_attachment = part
            .content_disposition()
            .map(|d| d.is_attachment())
            .unwrap_or(false);
        let mime = part
            .content_type()
            .map(|ct| match ct.subtype() {
                Some(sub) => format!("{}/{}", ct.ctype(), sub),
                None => ct.ctype().to_string(),
            })
            .unwrap_or_else(|| "application/octet-stream".into())
            .to_ascii_lowercase();
        let name = part.attachment_name().map(str::to_string);
        // Inline images without a name are decoration, not attachments.
        if name.is_none() && !disposition_attachment && !part.is_message() {
            continue;
        }
        let filename = name.unwrap_or_else(|| {
            if part.is_message() {
                "message.eml".into()
            } else {
                format!("attachment-{}", index + 1)
            }
        });
        out.push(AttachmentMeta {
            index: index as u32,
            filename,
            mime,
            size: part.contents().len(),
        });
    }
    out
}

pub fn html_to_text(html: &str) -> String {
    html2text::from_read(html.as_bytes(), 10_000).unwrap_or_else(|_| html.to_string())
}

/// The Bcc header, kept only in draft copies.
pub fn bcc(raw: &[u8]) -> Vec<Addr> {
    MessageParser::default()
        .parse(raw)
        .map(|m| addrs(m.bcc()))
        .unwrap_or_default()
}

/// Parses a full RFC 5322 message into what the UI shows.
pub fn parse_body(raw: &[u8]) -> ParsedBody {
    let Some(m) = MessageParser::default().parse(raw) else {
        let text = String::from_utf8_lossy(raw).into_owned();
        return ParsedBody {
            snippet: reading::snippet(&text),
            text,
            html: None,
            attachments: Vec::new(),
        };
    };
    let html = if m.html_body_count() > 0 {
        m.html_bodies()
            .find(|p| p.is_text_html())
            .and_then(|p| p.text_contents())
            .map(reading::sanitize_html)
    } else {
        None
    };
    let plain = m
        .text_bodies()
        .find(|p| p.is_text() && !p.is_text_html())
        .and_then(|p| p.text_contents())
        .map(|t| t.replace("\r\n", "\n"));
    let text = match (plain, &html) {
        (Some(t), _) if !t.trim().is_empty() => t,
        (_, Some(h)) => html_to_text(h),
        (Some(t), None) => t,
        (None, None) => String::new(),
    };
    ParsedBody {
        snippet: reading::snippet(&text),
        text,
        html,
        attachments: attachment_list(&m),
    }
}

/// Returns the bytes of the attachment at `index` (as listed by [`parse_body`]).
pub fn extract_attachment(raw: &[u8], index: u32) -> Option<Vec<u8>> {
    let m = MessageParser::default().parse(raw)?;
    m.attachment(index).map(|p| p.contents().to_vec())
}

/// Splits a recipients field typed by the user: `a@b.it, Nome <c@d.it>; e@f.it`.
pub fn parse_recipients(s: &str) -> Vec<Addr> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut in_angle = false;
    let mut flush = |cur: &mut String| {
        let t = cur.trim();
        if !t.is_empty() {
            if let Some(a) = parse_one(t) {
                out.push(a);
            }
        }
        cur.clear();
    };
    for c in s.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(c);
            }
            '<' if !in_quotes => {
                in_angle = true;
                cur.push(c);
            }
            '>' if !in_quotes => {
                in_angle = false;
                cur.push(c);
            }
            ',' | ';' if !in_quotes && !in_angle => flush(&mut cur),
            _ => cur.push(c),
        }
    }
    flush(&mut cur);
    out
}

fn parse_one(s: &str) -> Option<Addr> {
    if let (Some(l), Some(r)) = (s.rfind('<'), s.rfind('>')) {
        if l < r {
            let addr = s[l + 1..r].trim().to_string();
            let name = s[..l].trim().trim_matches('"').trim().to_string();
            return addr.contains('@').then_some(Addr { name, addr });
        }
    }
    let addr = s.trim().to_string();
    addr.contains('@').then_some(Addr {
        name: String::new(),
        addr,
    })
}
