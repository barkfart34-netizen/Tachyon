//! What the launcher keeps between starts (`launcher.json` in Tachyon's
//! folder): the offline accounts, the installations, and its settings.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// An offline account: a name, which gives the player's UUID
/// (`offline_uuid`) and default skin as vanilla's offline mode does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
}

impl Account {
    pub fn uuid(&self) -> u128 {
        horizon_protocol::crypto::offline_uuid(&self.name)
    }
    /// The UUID as vanilla writes it, with dashes.
    pub fn uuid_text(&self) -> String {
        let hex = format!("{:032x}", self.uuid());
        format!(
            "{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        )
    }
}

/// A player name vanilla accepts: 3 to 16 letters, digits or `_`.
pub fn valid_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A game folder with its own worlds, options and packs, and how the game
/// starts in it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Installation {
    pub id: String,
    pub name: String,
    /// The game folder; empty for Tachyon's own (`folder`).
    pub dir: String,
    /// The window's size at start; 0 for the game's own.
    pub width: u32,
    pub height: u32,
    /// The last start, in seconds since 1970; 0 before the first.
    pub last_played: u64,
}

/// The installation every launcher starts with.
pub const DEFAULT_ID: &str = "default";

impl Default for Installation {
    fn default() -> Self {
        Self {
            id: DEFAULT_ID.into(),
            name: "Tachyon".into(),
            dir: String::new(),
            width: 0,
            height: 0,
            last_played: 0,
        }
    }
}

impl Installation {
    /// The game folder: the chosen one, else `game` (the first installation)
    /// or `installations/<id>` in Tachyon's folder.
    pub fn folder(&self, data: &Path) -> PathBuf {
        if !self.dir.trim().is_empty() {
            PathBuf::from(self.dir.trim())
        } else {
            self.own_folder(data)
        }
    }
    pub fn own_folder(&self, data: &Path) -> PathBuf {
        if self.id == DEFAULT_ID {
            data.join("game")
        } else {
            data.join("installations").join(&self.id)
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub accounts: Vec<Account>,
    /// The name of the account that plays.
    pub active: String,
    pub installations: Vec<Installation>,
    /// The id of the installation PLAY starts.
    pub selected: String,
    /// The launcher stays on screen while the game runs.
    pub keep_open: bool,
    /// The panorama behind the Play tab stands still.
    pub still: bool,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            active: String::new(),
            installations: vec![Installation::default()],
            selected: DEFAULT_ID.into(),
            keep_open: false,
            still: false,
        }
    }
}

/// Seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Store {
    pub fn file(data: &Path) -> PathBuf {
        data.join("launcher.json")
    }

    pub fn load(data: &Path) -> Self {
        let mut store: Self = std::fs::read(Self::file(data))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        store.mend();
        store
    }

    /// At least one installation, ids that are unique, and a selected
    /// installation and active account that exist.
    fn mend(&mut self) {
        if self.installations.is_empty() {
            self.installations.push(Installation::default());
        }
        let mut seen = std::collections::HashSet::new();
        for (i, installation) in self.installations.iter_mut().enumerate() {
            if installation.id.is_empty() || !seen.insert(installation.id.clone()) {
                installation.id = format!("installation-{i}");
                seen.insert(installation.id.clone());
            }
        }
        if !self.installations.iter().any(|i| i.id == self.selected) {
            self.selected = self.installations[0].id.clone();
        }
        self.accounts.retain(|a| valid_name(&a.name));
        if !self.accounts.iter().any(|a| a.name == self.active) {
            self.active = self
                .accounts
                .first()
                .map(|a| a.name.clone())
                .unwrap_or_default();
        }
    }

    /// Writes the file whole (a new file renamed over the old one).
    pub fn save(&self, data: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(data)?;
        let file = Self::file(data);
        let partial = file.with_extension("json.partial");
        std::fs::write(&partial, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(partial, file)
    }

    pub fn account(&self) -> Option<&Account> {
        self.accounts.iter().find(|a| a.name == self.active)
    }

    pub fn selected_index(&self) -> usize {
        self.installations
            .iter()
            .position(|i| i.id == self.selected)
            .unwrap_or(0)
    }

    /// Adds `name` (or finds it, in any case) and makes it the active account.
    pub fn add_account(&mut self, name: &str) {
        match self
            .accounts
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
        {
            Some(account) => self.active = account.name.clone(),
            None => {
                self.accounts.push(Account { name: name.into() });
                self.active = name.into();
            }
        }
    }

    pub fn remove_account(&mut self, name: &str) {
        self.accounts.retain(|a| a.name != name);
        self.mend();
    }

    /// An id no installation has.
    pub fn new_id(&self) -> String {
        let mut n = self.installations.len();
        loop {
            let id = format!("installation-{n}");
            if !self.installations.iter().any(|i| i.id == id) {
                return id;
            }
            n += 1;
        }
    }

    pub fn remove_installation(&mut self, id: &str) {
        self.installations.retain(|i| i.id != id);
        self.mend();
    }
}

/// "3 minutes ago" for a time in seconds since 1970.
pub fn ago(then: u64, now: u64) -> String {
    let s = now.saturating_sub(then);
    let (n, unit) = match s {
        0..60 => return "Just now".into(),
        60..3600 => (s / 60, "minute"),
        3600..86400 => (s / 3600, "hour"),
        86400..2_592_000 => (s / 86400, "day"),
        2_592_000..31_536_000 => (s / 2_592_000, "month"),
        _ => (s / 31_536_000, "year"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_vanilla_names() {
        assert!(valid_name("Steve_01"));
        assert!(!valid_name("ab"));
        assert!(!valid_name("seventeen_letters"));
        assert!(!valid_name("no spaces"));
    }

    #[test]
    fn a_mended_store_has_what_it_points_at() {
        let mut store = Store {
            installations: Vec::new(),
            selected: "gone".into(),
            accounts: vec![
                Account { name: "x".into() },
                Account {
                    name: "Alex".into(),
                },
            ],
            active: "Steve".into(),
            ..Store::default()
        };
        store.mend();
        assert_eq!(store.installations.len(), 1);
        assert_eq!(store.selected, DEFAULT_ID);
        assert_eq!(
            store.accounts,
            vec![Account {
                name: "Alex".into()
            }]
        );
        assert_eq!(store.active, "Alex");
    }

    #[test]
    fn adding_an_account_twice_keeps_one() {
        let mut store = Store::default();
        store.add_account("Steve");
        store.add_account("steve");
        assert_eq!(store.accounts.len(), 1);
        assert_eq!(store.active, "Steve");
    }

    #[test]
    fn offline_uuids_have_vanilla_dashes() {
        let text = Account {
            name: "Steve".into(),
        }
        .uuid_text();
        assert_eq!(text.len(), 36);
        assert_eq!(text.matches('-').count(), 4);
    }

    #[test]
    fn times_read_as_words() {
        assert_eq!(ago(100, 130), "Just now");
        assert_eq!(ago(0, 7200), "2 hours ago");
        assert_eq!(ago(0, 86400), "1 day ago");
    }
}
