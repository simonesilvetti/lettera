#![allow(dead_code)]

use std::collections::BTreeMap;

use lettera_core::imap::{FolderStatus, ImapClient, RemoteFolder, RemoteMessage};
use lettera_core::Result;

pub fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

pub fn header_part(raw: &[u8]) -> Vec<u8> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(raw.len());
    raw[..end].to_vec()
}

#[derive(Default, Clone)]
pub struct MockFolder {
    pub attributes: Vec<String>,
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub messages: BTreeMap<u32, (String, Vec<u8>)>,
    /// Arrival dates (INTERNALDATE); missing means today.
    pub dates: BTreeMap<u32, chrono::NaiveDate>,
}

impl MockFolder {
    pub fn new(uidvalidity: u32) -> Self {
        MockFolder {
            uidvalidity,
            uidnext: 1,
            ..Default::default()
        }
    }

    pub fn add(&mut self, raw: Vec<u8>, flags: &str) -> u32 {
        let uid = self.uidnext;
        self.uidnext += 1;
        self.messages.insert(uid, (flags.to_string(), raw));
        uid
    }

    pub fn add_dated(&mut self, raw: Vec<u8>, flags: &str, date: chrono::NaiveDate) -> u32 {
        let uid = self.add(raw, flags);
        self.dates.insert(uid, date);
        uid
    }
}

/// In-memory IMAP server good enough for the sync logic.
#[derive(Default)]
pub struct MockImap {
    pub folders: BTreeMap<String, MockFolder>,
    pub capabilities: Vec<String>,
    pub selected: Option<String>,
    pub log: Vec<String>,
}

pub fn parse_set(set: &str, max: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for part in set.split(',') {
        match part.split_once(':') {
            Some((a, b)) => {
                let a: u32 = a.parse().unwrap_or(1);
                let b: u32 = if b == "*" {
                    max
                } else {
                    b.parse().unwrap_or(max)
                };
                let (lo, hi) = (a.min(b), a.max(b));
                out.extend(lo..=hi);
            }
            None => out.push(part.parse().unwrap_or(0)),
        }
    }
    out
}

impl MockImap {
    fn current(&mut self) -> &mut MockFolder {
        let name = self.selected.clone().expect("no folder selected");
        self.folders.get_mut(&name).expect("unknown folder")
    }

    fn uids(&mut self, set: &str) -> Vec<u32> {
        let f = self.current();
        let max = f.messages.keys().max().copied().unwrap_or(0);
        let wanted = parse_set(set, max);
        let mut v: Vec<u32> = f
            .messages
            .keys()
            .copied()
            .filter(|u| wanted.contains(u))
            .collect();
        // "N:*" always matches the highest message, as in RFC 3501
        if set.ends_with(":*") && v.is_empty() && max > 0 {
            v.push(max);
        }
        v
    }
}

impl ImapClient for MockImap {
    async fn list_folders(&mut self) -> Result<Vec<RemoteFolder>> {
        Ok(self
            .folders
            .iter()
            .map(|(name, f)| RemoteFolder {
                name: name.clone(),
                delimiter: Some("/".into()),
                attributes: f.attributes.clone(),
            })
            .collect())
    }

    async fn select(&mut self, folder: &str) -> Result<FolderStatus> {
        self.log.push(format!("SELECT {folder}"));
        let f = self
            .folders
            .get(folder)
            .ok_or_else(|| lettera_core::Error::Imap(format!("no folder {folder}")))?;
        let status = FolderStatus {
            uidvalidity: f.uidvalidity,
            uidnext: f.uidnext,
            exists: f.messages.len() as u32,
        };
        self.selected = Some(folder.to_string());
        Ok(status)
    }

    async fn fetch_headers(&mut self, uid_set: &str) -> Result<Vec<RemoteMessage>> {
        self.log.push(format!("FETCH HEADERS {uid_set}"));
        let uids = self.uids(uid_set);
        let f = self.current();
        Ok(uids
            .into_iter()
            .map(|u| {
                let (flags, raw) = &f.messages[&u];
                RemoteMessage {
                    uid: u,
                    flags: flags.clone(),
                    size: raw.len() as u32,
                    header: header_part(raw),
                }
            })
            .collect())
    }

    async fn fetch_flags(&mut self, uid_set: &str) -> Result<Vec<(u32, String)>> {
        self.log.push(format!("FETCH FLAGS {uid_set}"));
        let uids = self.uids(uid_set);
        let f = self.current();
        Ok(uids
            .into_iter()
            .map(|u| (u, f.messages[&u].0.clone()))
            .collect())
    }

    async fn search_since(&mut self, date: chrono::NaiveDate) -> Result<Vec<u32>> {
        self.log.push(format!("SEARCH SINCE {date}"));
        let today = chrono::Local::now().date_naive();
        let f = self.current();
        Ok(f.messages
            .keys()
            .copied()
            .filter(|u| f.dates.get(u).copied().unwrap_or(today) >= date)
            .collect())
    }

    async fn search_message_id(&mut self, id: &str) -> Result<Vec<u32>> {
        self.log.push(format!("SEARCH MESSAGE-ID {id}"));
        let f = self.current();
        Ok(f.messages
            .iter()
            .filter(|(_, (_, raw))| lettera_core::parse::parse_envelope(raw).message_id == id)
            .map(|(u, _)| *u)
            .collect())
    }

    async fn fetch_raw(&mut self, uid_set: &str) -> Result<Vec<(u32, Vec<u8>)>> {
        self.log.push(format!("FETCH RAW {uid_set}"));
        let uids = self.uids(uid_set);
        let f = self.current();
        Ok(uids
            .into_iter()
            .map(|u| (u, f.messages[&u].1.clone()))
            .collect())
    }

    async fn store(&mut self, uid_set: &str, add: bool, flags: &str) -> Result<()> {
        self.log.push(format!(
            "STORE {uid_set} {} {flags}",
            if add { "+" } else { "-" }
        ));
        let uids = self.uids(uid_set);
        let f = self.current();
        for u in uids {
            if let Some((fl, _)) = f.messages.get_mut(&u) {
                let mut set: Vec<String> = fl.split_whitespace().map(str::to_string).collect();
                for x in flags.split_whitespace() {
                    if add {
                        if !set.iter().any(|s| s == x) {
                            set.push(x.to_string());
                        }
                    } else {
                        set.retain(|s| s != x);
                    }
                }
                *fl = set.join(" ");
            }
        }
        Ok(())
    }

    async fn copy(&mut self, uid_set: &str, dest: &str) -> Result<()> {
        self.log.push(format!("COPY {uid_set} {dest}"));
        let uids = self.uids(uid_set);
        let msgs: Vec<_> = uids
            .iter()
            .map(|u| self.current().messages[u].clone())
            .collect();
        let d = self
            .folders
            .get_mut(dest)
            .ok_or_else(|| lettera_core::Error::Imap(format!("no folder {dest}")))?;
        for (flags, raw) in msgs {
            d.add(raw, &flags);
        }
        Ok(())
    }

    async fn mv(&mut self, uid_set: &str, dest: &str) -> Result<()> {
        self.log.push(format!("MOVE {uid_set} {dest}"));
        let uids = self.uids(uid_set);
        self.copy(uid_set, dest).await?;
        let f = self.current();
        for u in uids {
            f.messages.remove(&u);
        }
        Ok(())
    }

    async fn expunge(&mut self, uid_set: &str) -> Result<()> {
        self.log.push(format!("EXPUNGE {uid_set}"));
        let uids = self.uids(uid_set);
        let f = self.current();
        for u in uids {
            if f.messages
                .get(&u)
                .is_some_and(|(fl, _)| fl.contains("\\Deleted"))
            {
                f.messages.remove(&u);
            }
        }
        Ok(())
    }

    async fn append(&mut self, folder: &str, flags: &str, raw: &[u8]) -> Result<()> {
        self.log.push(format!("APPEND {folder}"));
        self.folders
            .get_mut(folder)
            .ok_or_else(|| lettera_core::Error::Imap(format!("no folder {folder}")))?
            .add(raw.to_vec(), flags);
        Ok(())
    }

    async fn create(&mut self, folder: &str) -> Result<()> {
        self.log.push(format!("CREATE {folder}"));
        self.folders
            .entry(folder.to_string())
            .or_insert_with(|| MockFolder::new(1));
        Ok(())
    }

    fn has_capability(&self, cap: &str) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case(cap))
    }

    fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
}
