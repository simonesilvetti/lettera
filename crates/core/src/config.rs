//! `~/.config/lettera/config.toml`: theme, font and the account list.
//! Secrets never live here; see [`crate::secrets`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_font")]
    pub font: String,
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    /// How many months of mail to keep in sync (by arrival date); 0 = everything.
    #[serde(default = "default_sync_months")]
    pub sync_months: u32,
    /// Appended to new messages, replies and forwards (after a "-- " line).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Start hidden in the tray with the desktop session (see [`crate::autostart`]).
    #[serde(default = "default_autostart")]
    pub autostart: bool,
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: default_theme(),
            font: default_font(),
            font_size: default_font_size(),
            sync_months: default_sync_months(),
            signature: None,
            autostart: default_autostart(),
            accounts: Vec::new(),
        }
    }
}

fn default_theme() -> String {
    "auto".into()
}
fn default_font() -> String {
    "JetBrains Mono".into()
}
fn default_font_size() -> u32 {
    13
}
fn default_sync_months() -> u32 {
    48
}
fn default_autostart() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Imap,
    Google,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: AccountKind,
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imap_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imap_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// "implicit" or "starttls"; only needed for non-standard ports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imap_tls: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp_tls: Option<String>,
    /// Overrides the global `sync_months` for this account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_months: Option<u32>,
    /// Overrides the global `signature`; an empty string means none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// Where and how to reach a server. TLS mode follows the port (993/465 are
/// implicit TLS, anything else STARTTLS) unless `imap_tls`/`smtp_tls` say otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub implicit_tls: bool,
}

fn tls_mode(setting: Option<&str>, default: bool) -> Result<bool> {
    match setting {
        None => Ok(default),
        Some("implicit") | Some("tls") => Ok(true),
        Some("starttls") => Ok(false),
        Some(other) => Err(Error::Config(format!(
            "invalid tls {other:?}: use \"implicit\" or \"starttls\""
        ))),
    }
}

impl Config {
    /// Sync window for an account in months, `None` meaning no limit.
    pub fn sync_months(&self, account: &AccountConfig) -> Option<u32> {
        Some(account.sync_months.unwrap_or(self.sync_months)).filter(|m| *m > 0)
    }

    /// Signature for an account, already trimmed; `None` if there is none.
    pub fn signature(&self, account: &AccountConfig) -> Option<String> {
        account
            .signature
            .as_ref()
            .or(self.signature.as_ref())
            .map(|s| s.trim_matches(['\n', '\r']).trim_end().to_string())
            .filter(|s| !s.trim().is_empty())
    }
}

/// Initial body of a draft: room to write, then the signature after the
/// conventional "-- " separator.
pub fn signed_body(signature: Option<&str>) -> String {
    match signature {
        Some(sig) => format!("\n\n-- \n{sig}"),
        None => String::new(),
    }
}

/// First day of the sync window: `months` months before `today`.
pub fn sync_since(today: chrono::NaiveDate, months: Option<u32>) -> Option<chrono::NaiveDate> {
    let m = months?;
    Some(
        today
            .checked_sub_months(chrono::Months::new(m))
            .unwrap_or(chrono::NaiveDate::MIN),
    )
}

impl AccountConfig {
    pub fn is_google(&self) -> bool {
        self.kind == AccountKind::Google
    }

    /// Gmail's label semantics apply: a google account, or plain IMAP on Gmail
    /// (app password).
    pub fn gmail_semantics(&self) -> bool {
        self.is_google()
            || self.imap_host.as_deref().is_some_and(|h| {
                h.eq_ignore_ascii_case("imap.gmail.com")
                    || h.eq_ignore_ascii_case("imap.googlemail.com")
            })
    }

    pub fn username(&self) -> &str {
        self.username.as_deref().unwrap_or(&self.email)
    }

    pub fn imap(&self) -> Result<Endpoint> {
        let (host, port) = match self.kind {
            AccountKind::Google => (
                self.imap_host
                    .clone()
                    .unwrap_or_else(|| "imap.gmail.com".into()),
                self.imap_port.unwrap_or(993),
            ),
            AccountKind::Imap => (
                self.imap_host.clone().ok_or_else(|| {
                    Error::Config(format!("{}: imap_host is missing", self.email))
                })?,
                self.imap_port.unwrap_or(993),
            ),
        };
        Ok(Endpoint {
            host,
            port,
            implicit_tls: tls_mode(self.imap_tls.as_deref(), port == 993)?,
        })
    }

    pub fn smtp(&self) -> Result<Endpoint> {
        let (host, port) = match self.kind {
            AccountKind::Google => (
                self.smtp_host
                    .clone()
                    .unwrap_or_else(|| "smtp.gmail.com".into()),
                self.smtp_port.unwrap_or(587),
            ),
            AccountKind::Imap => (
                self.smtp_host.clone().ok_or_else(|| {
                    Error::Config(format!("{}: smtp_host is missing", self.email))
                })?,
                self.smtp_port.unwrap_or(465),
            ),
        };
        Ok(Endpoint {
            host,
            port,
            implicit_tls: tls_mode(self.smtp_tls.as_deref(), port == 465)?,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if !self.email.contains('@') {
            return Err(Error::Config(format!("invalid email: {}", self.email)));
        }
        match self.kind {
            AccountKind::Imap => {
                self.imap()?;
                self.smtp()?;
            }
            AccountKind::Google => {
                if self.client_id.as_deref().unwrap_or("").is_empty()
                    || self.client_secret.as_deref().unwrap_or("").is_empty()
                {
                    return Err(Error::Config(format!(
                        "{}: client_id and client_secret are required for google",
                        self.email
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Filesystem locations. `LETTERA_HOME` puts config and data in one
/// directory (used by tests and for trying the app without touching ~).
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn detect() -> Result<Paths> {
        if let Ok(home) = std::env::var("LETTERA_HOME") {
            let home = PathBuf::from(home);
            return Ok(Paths {
                config_file: home.join("config.toml"),
                data_dir: home,
            });
        }
        let dirs = directories::ProjectDirs::from("", "", "lettera")
            .ok_or_else(|| Error::Config("cannot find the home directory".into()))?;
        Ok(Paths {
            config_file: dirs.config_dir().join("config.toml"),
            data_dir: dirs.data_dir().to_path_buf(),
        })
    }

    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("lettera.db")
    }
}

pub fn parse(text: &str) -> Result<Config> {
    let cfg: Config = toml::from_str(text)?;
    Ok(cfg)
}

/// Loads the config; a missing file is an empty config, a broken one is an error.
pub fn load(path: &Path) -> Result<Config> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.into()),
    }
}

const TEMPLATE: &str = r#"# lettera configuration. Reloaded automatically when you save it.
# Lines starting with # are comments: remove the # to turn an option on.

theme = "auto"            # auto | omarchy | dark | light
font = "JetBrains Mono"   # any installed monospace font
font_size = 13            # interface size in px; message bodies are 1px larger
sync_months = 48          # months of mail kept in sync, by arrival date (0 = all of it)
autostart = true          # start with the desktop session, hidden in the tray (false = only when opened)

# Signature added to new messages, replies and forwards, after a "-- " line.
# Each account can override it with its own `signature` ("" = none).
# signature = """
# Your Name
# +1 555 0100
# """

# Accounts are added from the app (":" then "add account") or by hand:
#
# [[accounts]]
# name = "Work"                    # shown in the app and used as sender name
# type = "imap"                    # imap | google
# email = "name@example.com"
# imap_host = "imap.example.com"
# imap_port = 993                  # 993 = implicit TLS, other ports = STARTTLS
# smtp_host = "smtp.example.com"
# smtp_port = 465                  # 465 = implicit TLS, other ports = STARTTLS
# username = "name@example.com"    # optional, defaults to the email
# imap_tls = "starttls"            # optional, for non-standard ports: implicit | starttls
# smtp_tls = "starttls"
# sync_months = 12                 # optional, overrides the global value
# signature = "Name · Work"        # optional, overrides the global signature
# client_id = "…"                  # google only, see the README
# client_secret = "…"              # google only
#
# Passwords and tokens live in the system keyring, never in this file.
"#;

/// Content of a fresh config file.
pub fn template() -> String {
    TEMPLATE.to_string()
}

fn toml_str(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn account_block(a: &AccountConfig) -> String {
    let mut out = String::from("\n[[accounts]]\n");
    out += &format!("name = {}\n", toml_str(&a.name));
    out += &format!(
        "type = \"{}\"\n",
        if a.is_google() { "google" } else { "imap" }
    );
    out += &format!("email = {}\n", toml_str(&a.email));
    if let Some(v) = &a.imap_host {
        out += &format!("imap_host = {}\n", toml_str(v));
    }
    if let Some(v) = a.imap_port {
        out += &format!("imap_port = {v}\n");
    }
    if let Some(v) = &a.smtp_host {
        out += &format!("smtp_host = {}\n", toml_str(v));
    }
    if let Some(v) = a.smtp_port {
        out += &format!("smtp_port = {v}\n");
    }
    if let Some(v) = &a.username {
        out += &format!("username = {}\n", toml_str(v));
    }
    if let Some(v) = &a.client_id {
        out += &format!("client_id = {}\n", toml_str(v));
    }
    if let Some(v) = &a.client_secret {
        out += &format!("client_secret = {}\n", toml_str(v));
    }
    if let Some(v) = &a.imap_tls {
        out += &format!("imap_tls = {}\n", toml_str(v));
    }
    if let Some(v) = &a.smtp_tls {
        out += &format!("smtp_tls = {}\n", toml_str(v));
    }
    // Optional settings that are not set yet, as commented placeholders.
    if !a.is_google() {
        if a.username.is_none() {
            out += "# username = \"name@example.com\"    # if the login differs from the email\n";
        }
        if a.imap_tls.is_none() {
            out += "# imap_tls = \"starttls\"            # implicit | starttls, for non-standard ports\n";
        }
        if a.smtp_tls.is_none() {
            out += "# smtp_tls = \"starttls\"\n";
        }
    }
    match a.sync_months {
        Some(v) => out += &format!("sync_months = {v}\n"),
        None => {
            out +=
                "# sync_months = 12                 # overrides the global value (0 = all mail)\n"
        }
    }
    match &a.signature {
        Some(v) => out += &format!("signature = {}\n", toml_str(v)),
        None => out +=
            "# signature = \"Name · Work\"        # overrides the global signature (\"\" = none)\n",
    }
    out += if a.is_google() {
        "# the refresh token lives in the system keyring, never in this file\n"
    } else {
        "# the password lives in the system keyring, never in this file\n"
    };
    out
}

/// Appends an account block to the file text, keeping the user's comments.
pub fn append_account(text: &str, account: &AccountConfig) -> String {
    let mut out = if text.trim().is_empty() {
        TEMPLATE.to_string()
    } else {
        text.to_string()
    };
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out += &account_block(account);
    out
}

/// Removes the `[[accounts]]` block whose email matches, keeping everything else verbatim.
pub fn remove_account(text: &str, email: &str) -> String {
    // Each section is a list of lines; comments right above a header belong to it.
    let mut sections: Vec<Vec<&str>> = vec![Vec::new()];
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with('[') {
            let prev = sections.last_mut().map(std::mem::take).unwrap_or_default();
            let keep = prev
                .iter()
                .rposition(|l| !l.trim_start().starts_with('#'))
                .map(|i| i + 1)
                .unwrap_or(0);
            if let Some(last) = sections.last_mut() {
                *last = prev[..keep].to_vec();
            }
            sections.push(prev[keep..].to_vec());
        }
        if let Some(last) = sections.last_mut() {
            last.push(line);
        }
    }
    sections
        .into_iter()
        .map(|lines| lines.concat())
        .filter(|s| {
            let body: String = s
                .lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n");
            let Some(rest) = body.trim_start().strip_prefix("[[accounts]]") else {
                return true;
            };
            let table: std::result::Result<toml::Table, _> = toml::from_str(rest);
            match table {
                Ok(t) => t.get("email").and_then(|v| v.as_str()) != Some(email),
                Err(_) => true,
            }
        })
        .collect()
}

pub fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        private_dir(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    restrict(&tmp, 0o600)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Creates `dir` if needed and makes it private to the user (0700 on Unix):
/// the config may hold an OAuth client secret, the data dir holds the mail.
pub fn private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    restrict(dir, 0o700)
}

/// Unix permissions for an existing file or directory; a no-op elsewhere.
pub fn restrict(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
theme = "auto"
font = "JetBrains Mono"
font_size = 13

[[accounts]]
name = "Work"
type = "imap"
email = "name@example.com"
imap_host = "imap.example.com"
imap_port = 993
smtp_host = "smtp.example.com"
smtp_port = 465
username = "name@example.com"

# comment to keep
[[accounts]]
name = "Gmail"
type = "google"
email = "name@gmail.com"
client_id = "id"
client_secret = "secret"
"#;

    #[test]
    fn parses_sample() {
        let c = parse(SAMPLE).unwrap();
        assert_eq!(c.accounts.len(), 2);
        assert_eq!(c.accounts[0].kind, AccountKind::Imap);
        assert!(c.accounts[0].imap().unwrap().implicit_tls);
        assert!(c.accounts[0].smtp().unwrap().implicit_tls);
        let g = &c.accounts[1];
        assert!(g.is_google());
        assert_eq!(g.imap().unwrap().host, "imap.gmail.com");
        let smtp = g.smtp().unwrap();
        assert_eq!((smtp.port, smtp.implicit_tls), (587, false));
        g.validate().unwrap();
    }

    #[test]
    fn defaults_when_empty() {
        let c = parse("").unwrap();
        assert_eq!(c, Config::default());
    }

    #[test]
    fn append_and_remove_roundtrip() {
        let acc = AccountConfig {
            name: "Home \"mine\"".into(),
            kind: AccountKind::Imap,
            email: "a@b.it".into(),
            imap_host: Some("imap.b.it".into()),
            imap_port: Some(143),
            smtp_host: Some("smtp.b.it".into()),
            smtp_port: Some(587),
            username: None,
            client_id: None,
            client_secret: None,
            imap_tls: Some("implicit".into()),
            smtp_tls: None,
            sync_months: Some(6),
            signature: Some("Home\n\"tel\" 123".into()),
        };
        let text = append_account(SAMPLE, &acc);
        let c = parse(&text).unwrap();
        assert_eq!(c.accounts.len(), 3);
        assert_eq!(c.accounts[2], acc);
        assert!(c.accounts[2].imap().unwrap().implicit_tls);
        assert!(!c.accounts[2].smtp().unwrap().implicit_tls);
        let back = remove_account(&text, "name@example.com");
        let c = parse(&back).unwrap();
        assert_eq!(c.accounts.len(), 2);
        assert!(back.contains("# comment to keep"));
        assert!(c.accounts.iter().all(|a| a.email != "name@example.com"));
    }

    #[test]
    fn append_to_empty_uses_template() {
        let acc = AccountConfig {
            name: "G".into(),
            kind: AccountKind::Google,
            email: "x@gmail.com".into(),
            imap_host: None,
            imap_port: None,
            smtp_host: None,
            smtp_port: None,
            username: None,
            client_id: Some("c".into()),
            client_secret: Some("s".into()),
            imap_tls: None,
            smtp_tls: None,
            sync_months: None,
            signature: None,
        };
        let text = append_account("", &acc);
        let c = parse(&text).unwrap();
        assert_eq!(c.theme, "auto");
        assert_eq!(c.accounts.len(), 1);
        assert_eq!(c.sync_months(&c.accounts[0]), Some(48));
    }

    #[test]
    fn gmail_over_plain_imap() {
        let c = parse("[[accounts]]\nname = \"g\"\ntype = \"imap\"\nemail = \"a@gmail.com\"\nimap_host = \"IMAP.gmail.com\"\nsmtp_host = \"smtp.gmail.com\"\n[[accounts]]\nname = \"o\"\ntype = \"imap\"\nemail = \"a@b.it\"\nimap_host = \"imap.b.it\"\nsmtp_host = \"smtp.b.it\"\n").unwrap();
        assert!(c.accounts[0].gmail_semantics() && !c.accounts[0].is_google());
        assert!(!c.accounts[1].gmail_semantics());
    }

    #[test]
    fn template_has_placeholders_and_survives_add_remove() {
        let fresh = template();
        assert_eq!(parse(&fresh).unwrap(), Config::default());
        assert!(fresh.contains("# signature = \"\"\""));
        let acc = AccountConfig {
            name: "Work".into(),
            kind: AccountKind::Imap,
            email: "w@example.com".into(),
            imap_host: Some("imap.example.com".into()),
            imap_port: None,
            smtp_host: Some("smtp.example.com".into()),
            smtp_port: None,
            username: None,
            client_id: None,
            client_secret: None,
            imap_tls: None,
            smtp_tls: None,
            sync_months: None,
            signature: None,
        };
        let added = append_account("", &acc);
        let c = parse(&added).unwrap();
        assert_eq!(c.accounts, vec![acc]);
        assert!(
            added.contains("# sync_months = 12") && added.contains("# smtp_tls = \"starttls\"")
        );
        // placeholders, once uncommented, are valid settings
        let at = added.rfind("# sync_months = 12").unwrap();
        let enabled = format!("{}{}", &added[..at], &added[at + 2..]);
        assert_eq!(parse(&enabled).unwrap().accounts[0].sync_months, Some(12));
        assert_eq!(
            remove_account(&added, "w@example.com").trim_end(),
            fresh.trim_end()
        );
    }

    #[cfg(unix)]
    #[test]
    fn config_and_dir_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("lettera-perm-{}", std::process::id()));
        let file = dir.join("sub").join("config.toml");
        write(&file, "theme = \"auto\"\n").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(file.parent().unwrap()), 0o700);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn signatures() {
        let c = parse("signature = \"\"\"\nSimone\n+39 333\n\"\"\"\n[[accounts]]\nname = \"a\"\ntype = \"google\"\nemail = \"a@b.it\"\n[[accounts]]\nname = \"b\"\ntype = \"google\"\nemail = \"b@b.it\"\nsignature = \"Office\"\n[[accounts]]\nname = \"c\"\ntype = \"google\"\nemail = \"c@b.it\"\nsignature = \"\"\n").unwrap();
        assert_eq!(
            c.signature(&c.accounts[0]).as_deref(),
            Some("Simone\n+39 333")
        );
        assert_eq!(c.signature(&c.accounts[1]).as_deref(), Some("Office"));
        assert_eq!(c.signature(&c.accounts[2]), None);
        assert_eq!(signed_body(Some("Office")), "\n\n-- \nOffice");
        assert_eq!(signed_body(None), "");
    }

    #[test]
    fn sync_window() {
        let mut c = parse("sync_months = 12\n[[accounts]]\nname = \"a\"\ntype = \"google\"\nemail = \"a@b.it\"\n[[accounts]]\nname = \"b\"\ntype = \"google\"\nemail = \"b@b.it\"\nsync_months = 0\n").unwrap();
        assert_eq!(c.sync_months(&c.accounts[0]), Some(12));
        assert_eq!(c.sync_months(&c.accounts[1]), None);
        c.sync_months = 0;
        assert_eq!(c.sync_months(&c.accounts[0]), None);
        let d = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(sync_since(d(2026, 10, 19), Some(48)), Some(d(2022, 10, 19)));
        assert_eq!(sync_since(d(2026, 3, 31), Some(1)), Some(d(2026, 2, 28)));
        assert_eq!(sync_since(d(2026, 3, 31), None), None);
    }
}
