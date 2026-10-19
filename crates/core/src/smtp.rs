//! Outgoing mail: reply/forward templates, RFC 5322 building and SMTP delivery.

use std::time::Duration;

use lettre::address::Envelope as SmtpEnvelope;
use lettre::transport::smtp::authentication::{Credentials as SmtpCredentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use mail_builder::headers::address::Address as BAddress;
use mail_builder::MessageBuilder;

use crate::config::Endpoint;
use crate::db::MessageRow;
use crate::error::{Error, Result};
use crate::imap::Credentials;
use crate::model::{Addr, DraftKind};

#[derive(Debug, Clone, Default)]
pub struct Outgoing {
    pub from: Addr,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub bcc: Vec<Addr>,
    pub subject: String,
    pub body: String,
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// (filename, mime, bytes)
    pub attachments: Vec<(String, String, Vec<u8>)>,
}

/// What a reply/forward starts with, computed from the original message.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub quote: String,
}

fn prefixed(prefix: &str, subject: &str) -> String {
    let lower = subject.trim().to_lowercase();
    let p = prefix.to_lowercase();
    if lower.starts_with(&format!("{p}:")) {
        subject.trim().to_string()
    } else {
        format!("{prefix}: {}", subject.trim())
    }
}

fn format_date(ts: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.format("%a, %b %-d, %Y at %H:%M").to_string())
        .unwrap_or_default()
}

fn list(a: &[Addr]) -> String {
    a.iter().map(Addr::full).collect::<Vec<_>>().join(", ")
}

fn dedup(mut v: Vec<Addr>, exclude: &[String]) -> Vec<Addr> {
    let mut seen: Vec<String> = exclude.iter().map(|e| e.to_lowercase()).collect();
    v.retain(|a| {
        let k = a.addr.to_lowercase();
        if seen.contains(&k) {
            false
        } else {
            seen.push(k);
            true
        }
    });
    v
}

/// Recipients, subject and quote for answering or forwarding `src`.
pub fn template(kind: DraftKind, src: &MessageRow, src_text: &str, me: &str) -> Template {
    let quoted = || {
        let mut q = format!("On {}, {} wrote:\n", format_date(src.date), src.from.full());
        for line in src_text.trim_end().lines() {
            if line.starts_with('>') {
                q.push('>');
            } else {
                q.push_str("> ");
            }
            q.push_str(line);
            q.push('\n');
        }
        q
    };
    // answering one's own message (from Sent, or a copy to oneself) goes
    // to its recipients again, as other clients do, not back to oneself
    let mine = src.from.addr.eq_ignore_ascii_case(me);
    let others = dedup(src.to.clone(), &[me.to_string()]);
    let reply_target = if !src.reply_to.is_empty() {
        src.reply_to.clone()
    } else if mine && !others.is_empty() {
        others
    } else {
        vec![src.from.clone()]
    };
    let me = vec![me.to_string()];
    match kind {
        DraftKind::New => Template {
            to: Vec::new(),
            cc: Vec::new(),
            subject: String::new(),
            quote: String::new(),
        },
        DraftKind::Reply => Template {
            to: reply_target,
            cc: Vec::new(),
            subject: prefixed("Re", &src.subject),
            quote: quoted(),
        },
        DraftKind::ReplyAll => {
            let mut to = reply_target;
            to.extend(src.to.iter().cloned());
            let to = dedup(to, &me);
            let exclude: Vec<String> = me
                .iter()
                .cloned()
                .chain(to.iter().map(|a| a.addr.clone()))
                .collect();
            Template {
                cc: dedup(src.cc.clone(), &exclude),
                to,
                subject: prefixed("Re", &src.subject),
                quote: quoted(),
            }
        }
        DraftKind::Forward => Template {
            to: Vec::new(),
            cc: Vec::new(),
            subject: prefixed("Fwd", &src.subject),
            quote: format!(
                "---------- Forwarded message ----------\nFrom: {}\nDate: {}\nSubject: {}\nTo: {}\n{}\n{}\n",
                src.from.full(),
                format_date(src.date),
                src.subject,
                list(&src.to),
                if src.cc.is_empty() {
                    String::new()
                } else {
                    format!("Cc: {}\n", list(&src.cc))
                },
                src_text.trim_end()
            ),
        },
    }
}

/// `In-Reply-To` and `References` for an answer to `src`.
pub fn thread_headers(src: &MessageRow) -> (Option<String>, Vec<String>) {
    let mut refs = src.refs.clone();
    if refs.is_empty() {
        if let Some(irt) = &src.in_reply_to {
            refs.push(irt.clone());
        }
    }
    if !refs.contains(&src.message_id) {
        refs.push(src.message_id.clone());
    }
    (Some(src.message_id.clone()), refs)
}

pub fn new_message_id(from: &str) -> String {
    let domain = from
        .rsplit('@')
        .next()
        .filter(|d| !d.is_empty())
        .unwrap_or("lettera.local");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let salt = crate::parse::fnv(format!("{nanos}{from}{:p}", &nanos).as_bytes());
    format!("{nanos:x}.{salt:x}@{domain}")
}

pub fn guess_mime(filename: &str) -> &'static str {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "json" => "application/json",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "eml" => "message/rfc822",
        "ics" => "text/calendar",
        _ => "application/octet-stream",
    }
}

fn baddr(a: &Addr) -> BAddress<'static> {
    if a.name.is_empty() {
        BAddress::from(a.addr.clone())
    } else {
        BAddress::from((a.name.clone(), a.addr.clone()))
    }
}

fn blist(v: &[Addr]) -> BAddress<'static> {
    BAddress::from(v.iter().map(baddr).collect::<Vec<_>>())
}

/// Builds the message as `text/plain`. Bcc recipients are not written in the headers.
pub fn build(o: &Outgoing) -> Result<Vec<u8>> {
    write(o, false)
}

/// The copy kept in the Drafts folder: like [`build`], plus the Bcc header.
pub fn build_draft(o: &Outgoing) -> Result<Vec<u8>> {
    write(o, true)
}

fn write(o: &Outgoing, with_bcc: bool) -> Result<Vec<u8>> {
    let mut b = MessageBuilder::new()
        .from(baddr(&o.from))
        .subject(o.subject.clone())
        .message_id(crate::parse::clean_id(&o.message_id))
        .date(chrono::Utc::now().timestamp())
        .text_body(o.body.clone());
    if !o.to.is_empty() {
        b = b.to(blist(&o.to));
    }
    if !o.cc.is_empty() {
        b = b.cc(blist(&o.cc));
    }
    if with_bcc && !o.bcc.is_empty() {
        b = b.bcc(blist(&o.bcc));
    }
    // ids come from received mail: cleaned again here, whatever stored them
    let ids = |v: &[String]| -> Vec<String> {
        v.iter()
            .map(|s| crate::parse::clean_id(s))
            .filter(|s| !s.is_empty())
            .collect()
    };
    if let Some(irt) = ids(o.in_reply_to.as_slice()).pop() {
        b = b.in_reply_to(irt);
    }
    let refs = ids(&o.references);
    if !refs.is_empty() {
        b = b.references(refs);
    }
    for (name, mime, bytes) in &o.attachments {
        b = b.attachment(mime.clone(), name.clone(), bytes.clone());
    }
    b.write_to_vec().map_err(|e| Error::Smtp(e.to_string()))
}

type Transport = AsyncSmtpTransport<Tokio1Executor>;

fn smtp_err(e: lettre::transport::smtp::Error) -> Error {
    Error::Smtp(e.to_string())
}

fn transport(ep: &Endpoint, creds: &Credentials, implicit_tls: bool) -> Result<Transport> {
    use lettre::transport::smtp::client::{Certificate, Tls, TlsParameters};
    let mut params = TlsParameters::builder(ep.host.clone());
    if let Ok(path) = std::env::var("LETTERA_CA_FILE") {
        let pem = std::fs::read(&path)?;
        params = params.add_root_certificate(Certificate::from_pem(&pem).map_err(smtp_err)?);
    }
    let params = params.build_rustls().map_err(smtp_err)?;
    let tls = if implicit_tls {
        Tls::Wrapper(params)
    } else {
        Tls::Required(params)
    };
    let (credentials, mechanisms) = match creds {
        Credentials::Password { username, password } => (
            SmtpCredentials::new(username.clone(), password.clone()),
            vec![Mechanism::Plain, Mechanism::Login],
        ),
        Credentials::XOAuth2 { user, access_token } => (
            SmtpCredentials::new(user.clone(), access_token.clone()),
            vec![Mechanism::Xoauth2],
        ),
    };
    Ok(Transport::builder_dangerous(&ep.host)
        .tls(tls)
        .port(ep.port)
        .credentials(credentials)
        .authentication(mechanisms)
        .timeout(Some(Duration::from_secs(60)))
        .build())
}

/// The server answered in clear text to a TLS handshake: it wants STARTTLS
/// on this port (typically 587 configured as implicit TLS).
pub fn is_plaintext_reply(e: &str) -> bool {
    e.contains("InvalidContentType") || e.contains("corrupt message")
}

/// Runs `op` on a transport for `ep`. If implicit TLS meets a server that
/// speaks clear text, retries once with STARTTLS (still mandatory TLS).
/// Also returns whether implicit TLS was the mode that worked.
async fn with_transport<T, F, Fut>(ep: &Endpoint, creds: &Credentials, op: F) -> Result<(T, bool)>
where
    F: Fn(Transport) -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, lettre::transport::smtp::Error>>,
{
    match op(transport(ep, creds, ep.implicit_tls)?).await {
        Ok(v) => Ok((v, ep.implicit_tls)),
        Err(e) if ep.implicit_tls && is_plaintext_reply(&e.to_string()) => {
            tracing::warn!(
                "{}:{} does not speak implicit TLS, retrying with STARTTLS (set smtp_tls = \"starttls\")",
                ep.host,
                ep.port
            );
            let v = op(transport(ep, creds, false)?).await.map_err(smtp_err)?;
            Ok((v, false))
        }
        Err(e) => Err(smtp_err(e)),
    }
}

/// Connects and authenticates without sending anything. Returns whether
/// the server is on implicit TLS (`false`: STARTTLS).
pub async fn check(ep: &Endpoint, creds: &Credentials) -> Result<bool> {
    let (ok, implicit) =
        with_transport(ep, creds, |t| async move { t.test_connection().await }).await?;
    if !ok {
        return Err(Error::Smtp(format!(
            "{}:{} does not answer",
            ep.host, ep.port
        )));
    }
    Ok(implicit)
}

pub async fn send(ep: &Endpoint, creds: &Credentials, o: &Outgoing, raw: &[u8]) -> Result<()> {
    let parse = |a: &str| {
        a.parse::<lettre::Address>()
            .map_err(|e| Error::Smtp(format!("invalid address {a}: {e}")))
    };
    let rcpts =
        o.to.iter()
            .chain(&o.cc)
            .chain(&o.bcc)
            .map(|a| parse(&a.addr))
            .collect::<Result<Vec<_>>>()?;
    if rcpts.is_empty() {
        return Err(Error::Smtp("no recipients".into()));
    }
    let envelope = SmtpEnvelope::new(Some(parse(&o.from.addr)?), rcpts)
        .map_err(|e| Error::Smtp(e.to_string()))?;
    with_transport(ep, creds, |t| {
        let envelope = envelope.clone();
        async move { t.send_raw(&envelope, raw).await }
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_reply_detection() {
        assert!(is_plaintext_reply(
            "Connection error: received corrupt message of type InvalidContentType"
        ));
        assert!(!is_plaintext_reply(
            "permanent error (535): bad credentials"
        ));
    }

    fn src() -> MessageRow {
        MessageRow {
            id: 1,
            account_id: 1,
            folder_id: 1,
            uid: 1,
            message_id: "orig@x.it".into(),
            in_reply_to: Some("root@x.it".into()),
            refs: vec!["root@x.it".into()],
            from: Addr {
                name: "Anna".into(),
                addr: "anna@x.it".into(),
            },
            reply_to: vec![],
            to: vec![
                Addr {
                    name: String::new(),
                    addr: "me@y.it".into(),
                },
                Addr {
                    name: "Bruno".into(),
                    addr: "bruno@x.it".into(),
                },
            ],
            cc: vec![Addr {
                name: String::new(),
                addr: "carla@x.it".into(),
            }],
            subject: "Re: piano".into(),
            date: 1_700_000_000,
            flags: String::new(),
            thread_id: "1:root@x.it".into(),
        }
    }

    #[test]
    fn reply_all_excludes_me() {
        let t = template(DraftKind::ReplyAll, &src(), "ciao\n> vecchio", "ME@y.it");
        let to: Vec<_> = t.to.iter().map(|a| a.addr.as_str()).collect();
        assert_eq!(to, ["anna@x.it", "bruno@x.it"]);
        assert_eq!(t.cc.len(), 1);
        assert_eq!(t.subject, "Re: piano");
        assert!(t.quote.contains("> ciao\n>> vecchio"));
        assert!(t.quote.contains("Anna <anna@x.it> wrote:"));
    }

    #[test]
    fn reply_to_my_own_message_goes_to_its_recipients() {
        let mut s = src();
        s.from = Addr {
            name: "Me".into(),
            addr: "me@y.it".into(),
        };
        let t = template(DraftKind::Reply, &s, "x", "ME@y.it");
        let to: Vec<_> = t.to.iter().map(|a| a.addr.as_str()).collect();
        assert_eq!(to, ["bruno@x.it"]);
        let t = template(DraftKind::ReplyAll, &s, "x", "ME@y.it");
        let to: Vec<_> = t.to.iter().map(|a| a.addr.as_str()).collect();
        assert_eq!(to, ["bruno@x.it"]);
        assert_eq!(t.cc.len(), 1);
    }

    #[test]
    fn forward_subject() {
        let t = template(DraftKind::Forward, &src(), "corpo", "me@y.it");
        assert_eq!(t.subject, "Fwd: Re: piano");
        assert!(t.to.is_empty());
        assert!(t.quote.contains("corpo"));
    }

    #[test]
    fn headers_chain() {
        let (irt, refs) = thread_headers(&src());
        assert_eq!(irt.as_deref(), Some("orig@x.it"));
        assert_eq!(refs, ["root@x.it", "orig@x.it"]);
    }

    #[test]
    fn built_message_has_thread_headers_and_no_bcc() {
        let o = Outgoing {
            from: Addr {
                name: "Me".into(),
                addr: "me@y.it".into(),
            },
            to: vec![Addr {
                name: String::new(),
                addr: "anna@x.it".into(),
            }],
            bcc: vec![Addr {
                name: String::new(),
                addr: "secret@z.it".into(),
            }],
            subject: "Re: piano".into(),
            body: "ok".into(),
            message_id: "new@y.it".into(),
            in_reply_to: Some("orig@x.it".into()),
            references: vec!["root@x.it".into(), "orig@x.it".into()],
            ..Default::default()
        };
        let raw = String::from_utf8(build(&o).unwrap()).unwrap();
        assert!(raw.contains("In-Reply-To: <orig@x.it>"));
        assert!(raw.contains("References: <root@x.it> <orig@x.it>"));
        assert!(raw.contains("Message-ID: <new@y.it>"));
        assert!(!raw.contains("secret@z.it"));
        assert!(raw.contains("text/plain"));
    }

    #[test]
    fn header_injection_is_neutralised() {
        let a = |n: &str, e: &str| Addr {
            name: n.into(),
            addr: e.into(),
        };
        let o = Outgoing {
            from: a("Me\r\nBcc: spy@evil.example", "me@x.it"),
            to: vec![a("Anna", "anna@x.it")],
            cc: vec![],
            bcc: vec![a("", "hidden@x.it")],
            subject: "hi\r\nBcc: spy@evil.example\r\nX-Evil: 1".into(),
            body: "body\r\n.\r\nMAIL FROM:<x@y>".into(),
            message_id: "id@x.it".into(),
            in_reply_to: Some("a@x.it>\r\nX-Evil: 2".into()),
            references: vec![],
            attachments: vec![(
                "a\r\nX-Evil: 3.txt".into(),
                "text/plain".into(),
                b"x".to_vec(),
            )],
        };
        let raw = String::from_utf8(build(&o).unwrap()).unwrap();
        let headers: Vec<&str> = raw.split("\r\n\r\n").next().unwrap().lines().collect();
        for h in &headers {
            let name = h.split(':').next().unwrap_or("").to_ascii_lowercase();
            assert!(
                name != "bcc" && name != "x-evil",
                "injected header {h:?}\n{raw}"
            );
        }
        assert!(
            !raw.to_ascii_lowercase().contains("hidden@x.it"),
            "Bcc never in headers"
        );
        let parsed = mail_parser::MessageParser::default()
            .parse(raw.as_bytes())
            .unwrap();
        assert!(parsed.header("X-Evil").is_none() && parsed.bcc().is_none());
    }
}
