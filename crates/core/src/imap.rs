//! IMAP access behind a small trait, so sync logic can be tested with a mock.

use std::future::Future;
use std::sync::Arc;

use async_imap::types::{Fetch, Flag, NameAttribute};
use futures::TryStreamExt;
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

use crate::config::Endpoint;
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteFolder {
    pub name: String,
    pub delimiter: Option<String>,
    /// Lowercased attributes, e.g. `\sent`, `\noselect`.
    pub attributes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FolderStatus {
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub exists: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteMessage {
    pub uid: u32,
    pub flags: String,
    pub size: u32,
    pub header: Vec<u8>,
}

/// The handful of IMAP operations Lettera needs. UID-based throughout.
pub trait ImapClient: Send {
    fn list_folders(&mut self) -> impl Future<Output = Result<Vec<RemoteFolder>>> + Send;
    fn select(&mut self, folder: &str) -> impl Future<Output = Result<FolderStatus>> + Send;
    /// `UID FETCH <set> (UID FLAGS RFC822.SIZE BODY.PEEK[HEADER])` on the selected folder.
    fn fetch_headers(
        &mut self,
        uid_set: &str,
    ) -> impl Future<Output = Result<Vec<RemoteMessage>>> + Send;
    /// `UID FETCH <set> (UID FLAGS)` on the selected folder.
    fn fetch_flags(
        &mut self,
        uid_set: &str,
    ) -> impl Future<Output = Result<Vec<(u32, String)>>> + Send;
    /// `UID SEARCH SINCE <date>` on the selected folder (by arrival date).
    fn search_since(
        &mut self,
        date: chrono::NaiveDate,
    ) -> impl Future<Output = Result<Vec<u32>>> + Send;
    /// Full RFC 5322 bytes for each UID in the set.
    fn fetch_raw(
        &mut self,
        uid_set: &str,
    ) -> impl Future<Output = Result<Vec<(u32, Vec<u8>)>>> + Send;
    fn store(
        &mut self,
        uid_set: &str,
        add: bool,
        flags: &str,
    ) -> impl Future<Output = Result<()>> + Send;
    fn copy(&mut self, uid_set: &str, dest: &str) -> impl Future<Output = Result<()>> + Send;
    fn mv(&mut self, uid_set: &str, dest: &str) -> impl Future<Output = Result<()>> + Send;
    fn expunge(&mut self, uid_set: &str) -> impl Future<Output = Result<()>> + Send;
    fn append(
        &mut self,
        folder: &str,
        flags: &str,
        raw: &[u8],
    ) -> impl Future<Output = Result<()>> + Send;
    fn create(&mut self, folder: &str) -> impl Future<Output = Result<()>> + Send;
    fn has_capability(&self, cap: &str) -> bool;
    fn selected(&self) -> Option<&str>;
}

/// Maps a folder to a role using SPECIAL-USE (RFC 6154), then well-known names.
pub fn folder_role(f: &RemoteFolder) -> Option<&'static str> {
    if f.name.eq_ignore_ascii_case("INBOX") {
        return Some("inbox");
    }
    for a in &f.attributes {
        let role = match a.as_str() {
            "\\sent" => "sent",
            "\\trash" => "trash",
            "\\archive" => "archive",
            "\\drafts" => "drafts",
            "\\junk" => "junk",
            "\\all" => "all",
            _ => continue,
        };
        return Some(role);
    }
    let leaf = match &f.delimiter {
        Some(d) if !d.is_empty() => f.name.rsplit(d.as_str()).next().unwrap_or(&f.name),
        _ => f.name.as_str(),
    }
    .trim()
    .to_lowercase();
    let role = match leaf.as_str() {
        "sent" | "sent items" | "sent messages" | "sent mail" | "inviati" | "posta inviata"
        | "elementi inviati" | "messaggi inviati" => "sent",
        "trash" | "deleted" | "deleted items" | "deleted messages" | "bin" | "cestino"
        | "posta eliminata" | "elementi eliminati" => "trash",
        "archive" | "archives" | "archivio" | "archiviati" => "archive",
        "drafts" | "draft" | "bozze" => "drafts",
        "junk" | "spam" | "junk e-mail" | "junk email" | "bulk mail" | "posta indesiderata" => {
            "junk"
        }
        "all mail" | "tutti i messaggi" => "all",
        _ => return None,
    };
    Some(role)
}

/// Turns a failed LOGIN into a message the user can act on.
pub fn login_error(server: &str) -> String {
    if server.contains("Application-specific password required") || server.contains("185833") {
        return "Gmail non accetta la password dell'account via IMAP: crea una password per le app \
                su https://myaccount.google.com/apppasswords (serve la verifica in due passaggi) \
                e usala qui, oppure aggiungi l'account come tipo google (OAuth)."
            .into();
    }
    format!("login IMAP fallito: {server}")
}

pub fn is_selectable(f: &RemoteFolder) -> bool {
    !f.attributes
        .iter()
        .any(|a| a == "\\noselect" || a == "\\nonexistent")
}

fn flag_str(f: &Flag) -> String {
    match f {
        Flag::Seen => "\\Seen".into(),
        Flag::Answered => "\\Answered".into(),
        Flag::Flagged => "\\Flagged".into(),
        Flag::Deleted => "\\Deleted".into(),
        Flag::Draft => "\\Draft".into(),
        Flag::Recent => "\\Recent".into(),
        Flag::MayCreate => "\\*".into(),
        Flag::Custom(c) => c.to_string(),
    }
}

fn flags_of(f: &Fetch) -> String {
    f.flags()
        .filter(|f| !matches!(f, Flag::Recent))
        .map(|f| flag_str(&f))
        .collect::<Vec<_>>()
        .join(" ")
}

fn attr_str(a: &NameAttribute) -> String {
    match a {
        NameAttribute::NoInferiors => "\\noinferiors".into(),
        NameAttribute::NoSelect => "\\noselect".into(),
        NameAttribute::Marked => "\\marked".into(),
        NameAttribute::Unmarked => "\\unmarked".into(),
        NameAttribute::All => "\\all".into(),
        NameAttribute::Archive => "\\archive".into(),
        NameAttribute::Drafts => "\\drafts".into(),
        NameAttribute::Flagged => "\\flagged".into(),
        NameAttribute::Junk => "\\junk".into(),
        NameAttribute::Sent => "\\sent".into(),
        NameAttribute::Trash => "\\trash".into(),
        other => format!("{other:?}").to_lowercase(),
    }
}

/// How to log in.
#[derive(Clone)]
pub enum Credentials {
    Password { username: String, password: String },
    XOAuth2 { user: String, access_token: String },
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Credentials::Password { username, .. } => write!(f, "Password({username}, ***)"),
            Credentials::XOAuth2 { user, .. } => write!(f, "XOAuth2({user}, ***)"),
        }
    }
}

/// SASL XOAUTH2 initial response: `user=…\x01auth=Bearer …\x01\x01`.
pub fn xoauth2_string(user: &str, token: &str) -> String {
    format!("user={user}\x01auth=Bearer {token}\x01\x01")
}

struct XOAuth2 {
    payload: String,
    sent: bool,
}

impl async_imap::Authenticator for XOAuth2 {
    type Response = String;
    fn process(&mut self, _challenge: &[u8]) -> String {
        // After a failure Google sends a JSON challenge; an empty reply ends the exchange.
        if self.sent {
            String::new()
        } else {
            self.sent = true;
            self.payload.clone()
        }
    }
}

pub type Session = async_imap::Session<TlsStream<TcpStream>>;

/// Trusted roots: Mozilla's bundle plus, if set, the PEM certificates in
/// `LETTERA_CA_FILE` (for servers with a private CA).
pub fn root_store() -> rustls::RootCertStore {
    let mut roots =
        rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Ok(path) = std::env::var("LETTERA_CA_FILE") {
        use rustls::pki_types::pem::PemObject;
        match rustls::pki_types::CertificateDer::pem_file_iter(&path) {
            Ok(certs) => {
                for cert in certs.flatten() {
                    if let Err(e) = roots.add(cert) {
                        tracing::warn!("LETTERA_CA_FILE: {e}");
                    }
                }
            }
            Err(e) => tracing::warn!("LETTERA_CA_FILE {path}: {e}"),
        }
    }
    roots
}

pub fn tls_config() -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    match rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
    {
        Ok(b) => b.with_root_certificates(root_store()).with_no_client_auth(),
        Err(_) => rustls::ClientConfig::builder()
            .with_root_certificates(root_store())
            .with_no_client_auth(),
    }
}

pub fn tls_connector() -> TlsConnector {
    TlsConnector::from(Arc::new(tls_config()))
}

async fn tls(tls: &TlsConnector, stream: TcpStream, host: &str) -> Result<TlsStream<TcpStream>> {
    let name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|e| Error::Imap(format!("host non valido {host}: {e}")))?;
    Ok(tls.connect(name, stream).await?)
}

/// Opens an authenticated session (implicit TLS or STARTTLS, depending on the port).
pub async fn connect(ep: &Endpoint, creds: &Credentials) -> Result<RealImap> {
    connect_with(ep, creds, &tls_connector()).await
}

/// Same as [`connect`] with a custom TLS configuration (tests use their own CA).
/// The whole handshake (TCP, TLS, greeting, login) is bounded by a timeout.
pub async fn connect_with(
    ep: &Endpoint,
    creds: &Credentials,
    connector: &TlsConnector,
) -> Result<RealImap> {
    tokio::time::timeout(
        std::time::Duration::from_secs(45),
        open(ep, creds, connector),
    )
    .await
    .map_err(|_| {
        Error::Imap(format!(
            "nessuna risposta da {}:{} ({}): controlla porta e modalità tls",
            ep.host,
            ep.port,
            if ep.implicit_tls {
                "tls implicito"
            } else {
                "starttls"
            }
        ))
    })?
}

async fn open(ep: &Endpoint, creds: &Credentials, connector: &TlsConnector) -> Result<RealImap> {
    let tcp = TcpStream::connect((ep.host.as_str(), ep.port)).await?;
    let stream = if ep.implicit_tls {
        tls(connector, tcp, &ep.host).await?
    } else {
        let mut plain = async_imap::Client::new(tcp);
        plain
            .read_response()
            .await?
            .ok_or_else(|| Error::Imap("nessun saluto dal server".into()))?;
        plain.run_command_and_check_ok("STARTTLS", None).await?;
        tls(connector, plain.into_inner(), &ep.host).await?
    };
    let mut client = async_imap::Client::new(stream);
    if ep.implicit_tls {
        client
            .read_response()
            .await?
            .ok_or_else(|| Error::Imap("nessun saluto dal server".into()))?;
    }
    let session = match creds {
        Credentials::Password { username, password } => client
            .login(username, password)
            .await
            .map_err(|(e, _)| Error::Auth(login_error(&e.to_string())))?,
        Credentials::XOAuth2 { user, access_token } => client
            .authenticate(
                "XOAUTH2",
                XOAuth2 {
                    payload: xoauth2_string(user, access_token),
                    sent: false,
                },
            )
            .await
            .map_err(|(e, _)| Error::Auth(format!("XOAUTH2 IMAP fallito: {e}")))?,
    };
    RealImap::new(session).await
}

pub struct RealImap {
    pub session: Session,
    caps: async_imap::types::Capabilities,
    selected: Option<String>,
}

impl RealImap {
    async fn new(mut session: Session) -> Result<RealImap> {
        let caps = session.capabilities().await?;
        Ok(RealImap {
            session,
            caps,
            selected: None,
        })
    }

    pub fn into_session(self) -> Session {
        self.session
    }

    pub async fn logout(mut self) {
        let _ = self.session.logout().await;
    }

    pub async fn noop(&mut self) -> Result<()> {
        Ok(self.session.noop().await?)
    }
}

impl ImapClient for RealImap {
    async fn list_folders(&mut self) -> Result<Vec<RemoteFolder>> {
        let names: Vec<_> = self
            .session
            .list(Some(""), Some("*"))
            .await?
            .try_collect()
            .await?;
        Ok(names
            .iter()
            .map(|n| RemoteFolder {
                name: n.name().to_string(),
                delimiter: n.delimiter().map(str::to_string),
                attributes: n.attributes().iter().map(attr_str).collect(),
            })
            .collect())
    }

    async fn select(&mut self, folder: &str) -> Result<FolderStatus> {
        let mb = self.session.select(folder).await?;
        self.selected = Some(folder.to_string());
        Ok(FolderStatus {
            uidvalidity: mb.uid_validity.unwrap_or(0),
            uidnext: mb.uid_next.unwrap_or(0),
            exists: mb.exists,
        })
    }

    async fn fetch_headers(&mut self, uid_set: &str) -> Result<Vec<RemoteMessage>> {
        let fetches: Vec<Fetch> = self
            .session
            .uid_fetch(uid_set, "(UID FLAGS RFC822.SIZE BODY.PEEK[HEADER])")
            .await?
            .try_collect()
            .await?;
        Ok(fetches
            .iter()
            .filter_map(|f| {
                Some(RemoteMessage {
                    uid: f.uid?,
                    flags: flags_of(f),
                    size: f.size.unwrap_or(0),
                    header: f.header().unwrap_or_default().to_vec(),
                })
            })
            .collect())
    }

    async fn fetch_flags(&mut self, uid_set: &str) -> Result<Vec<(u32, String)>> {
        let fetches: Vec<Fetch> = self
            .session
            .uid_fetch(uid_set, "(UID FLAGS)")
            .await?
            .try_collect()
            .await?;
        Ok(fetches
            .iter()
            .filter_map(|f| Some((f.uid?, flags_of(f))))
            .collect())
    }

    async fn search_since(&mut self, date: chrono::NaiveDate) -> Result<Vec<u32>> {
        let query = format!("SINCE {}", date.format("%d-%b-%Y"));
        Ok(self.session.uid_search(query).await?.into_iter().collect())
    }

    async fn fetch_raw(&mut self, uid_set: &str) -> Result<Vec<(u32, Vec<u8>)>> {
        let fetches: Vec<Fetch> = self
            .session
            .uid_fetch(uid_set, "(UID BODY.PEEK[])")
            .await?
            .try_collect()
            .await?;
        Ok(fetches
            .iter()
            .filter_map(|f| Some((f.uid?, f.body()?.to_vec())))
            .collect())
    }

    async fn store(&mut self, uid_set: &str, add: bool, flags: &str) -> Result<()> {
        let op = if add {
            "+FLAGS.SILENT"
        } else {
            "-FLAGS.SILENT"
        };
        let _: Vec<Fetch> = self
            .session
            .uid_store(uid_set, format!("{op} ({flags})"))
            .await?
            .try_collect()
            .await?;
        Ok(())
    }

    async fn copy(&mut self, uid_set: &str, dest: &str) -> Result<()> {
        Ok(self.session.uid_copy(uid_set, dest).await?)
    }

    async fn mv(&mut self, uid_set: &str, dest: &str) -> Result<()> {
        Ok(self.session.uid_mv(uid_set, dest).await?)
    }

    async fn expunge(&mut self, uid_set: &str) -> Result<()> {
        if self.has_capability("UIDPLUS") {
            let _: Vec<u32> = self
                .session
                .uid_expunge(uid_set)
                .await?
                .try_collect()
                .await?;
        } else {
            let _: Vec<u32> = self.session.expunge().await?.try_collect().await?;
        }
        Ok(())
    }

    async fn append(&mut self, folder: &str, flags: &str, raw: &[u8]) -> Result<()> {
        let flags = format!("({flags})");
        Ok(self
            .session
            .append(folder, Some(flags.as_str()), None, raw)
            .await?)
    }

    async fn create(&mut self, folder: &str) -> Result<()> {
        Ok(self.session.create(folder).await?)
    }

    fn has_capability(&self, cap: &str) -> bool {
        self.caps.has_str(cap)
    }

    fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_app_password_hint() {
        let raw = "no response: code: Some(Alert), info: Some(\"Application-specific password required: https://support.google.com/accounts/answer/185833 (Failure)\")";
        assert!(login_error(raw).contains("apppasswords"));
        assert!(login_error("NO [AUTHENTICATIONFAILED]").starts_with("login IMAP fallito"));
    }

    fn f(name: &str, attrs: &[&str]) -> RemoteFolder {
        RemoteFolder {
            name: name.into(),
            delimiter: Some("/".into()),
            attributes: attrs.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn roles() {
        assert_eq!(folder_role(&f("INBOX", &[])), Some("inbox"));
        assert_eq!(
            folder_role(&f("[Gmail]/Posta inviata", &["\\sent"])),
            Some("sent")
        );
        assert_eq!(
            folder_role(&f("[Gmail]/Tutti i messaggi", &["\\all"])),
            Some("all")
        );
        assert_eq!(
            folder_role(&f("[Gmail]/Cestino", &["\\hasnochildren", "\\trash"])),
            Some("trash")
        );
        assert_eq!(folder_role(&f("Posta inviata", &[])), Some("sent"));
        assert_eq!(folder_role(&f("INBOX/Cestino", &[])), Some("trash"));
        assert_eq!(folder_role(&f("Archivio", &[])), Some("archive"));
        assert_eq!(folder_role(&f("Bozze", &[])), Some("drafts"));
        assert_eq!(folder_role(&f("Spam", &[])), Some("junk"));
        assert_eq!(folder_role(&f("Progetti", &[])), None);
        assert!(!is_selectable(&f("[Gmail]", &["\\noselect"])));
    }

    #[test]
    fn xoauth2_format() {
        assert_eq!(
            xoauth2_string("a@gmail.com", "tok"),
            "user=a@gmail.com\x01auth=Bearer tok\x01\x01"
        );
    }
}
