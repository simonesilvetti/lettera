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
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: default_theme(),
            font: default_font(),
            font_size: default_font_size(),
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
}

/// Where and how to reach a server. TLS mode follows the port:
/// 993/465 are implicit TLS, anything else uses STARTTLS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub implicit_tls: bool,
}

impl AccountConfig {
    pub fn is_google(&self) -> bool {
        self.kind == AccountKind::Google
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
                self.imap_host
                    .clone()
                    .ok_or_else(|| Error::Config(format!("{}: imap_host mancante", self.email)))?,
                self.imap_port.unwrap_or(993),
            ),
        };
        Ok(Endpoint {
            host,
            port,
            implicit_tls: port == 993,
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
                self.smtp_host
                    .clone()
                    .ok_or_else(|| Error::Config(format!("{}: smtp_host mancante", self.email)))?,
                self.smtp_port.unwrap_or(465),
            ),
        };
        Ok(Endpoint {
            host,
            port,
            implicit_tls: port == 465,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if !self.email.contains('@') {
            return Err(Error::Config(format!("email non valida: {}", self.email)));
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
                        "{}: client_id e client_secret sono obbligatori per google",
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
            .ok_or_else(|| Error::Config("impossibile trovare la home".into()))?;
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

const TEMPLATE: &str = r#"# lettera — configurazione. Ricaricata automaticamente quando la salvi.
theme = "auto"        # auto | omarchy | dark | light
font = "JetBrains Mono"
font_size = 13
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
    if !a.is_google() {
        out += "# la password sta nel keyring di sistema, mai nel file\n";
    }
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
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
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
name = "Lavoro"
type = "imap"
email = "nome@dominio.it"
imap_host = "imap.dominio.it"
imap_port = 993
smtp_host = "smtp.dominio.it"
smtp_port = 465
username = "nome@dominio.it"

# commento da preservare
[[accounts]]
name = "Gmail"
type = "google"
email = "nome@gmail.com"
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
            name: "Casa \"mia\"".into(),
            kind: AccountKind::Imap,
            email: "a@b.it".into(),
            imap_host: Some("imap.b.it".into()),
            imap_port: Some(143),
            smtp_host: Some("smtp.b.it".into()),
            smtp_port: Some(587),
            username: None,
            client_id: None,
            client_secret: None,
        };
        let text = append_account(SAMPLE, &acc);
        let c = parse(&text).unwrap();
        assert_eq!(c.accounts.len(), 3);
        assert_eq!(c.accounts[2], acc);
        let back = remove_account(&text, "nome@dominio.it");
        let c = parse(&back).unwrap();
        assert_eq!(c.accounts.len(), 2);
        assert!(back.contains("# commento da preservare"));
        assert!(c.accounts.iter().all(|a| a.email != "nome@dominio.it"));
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
        };
        let text = append_account("", &acc);
        let c = parse(&text).unwrap();
        assert_eq!(c.theme, "auto");
        assert_eq!(c.accounts.len(), 1);
    }
}
