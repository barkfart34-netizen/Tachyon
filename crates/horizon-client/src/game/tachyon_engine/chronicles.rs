//! Chronicles, the modpack's quests (docs/tachyon/tce.md). The quests are
//! data: `chronicles/data/chronicles/tachyon/quests.json` in the built-in
//! content, written by tachyon/art/chronicles.py, and read by every game of
//! the process alike.
//!
//! The world's owner keeps each player's progress in the world's folder
//! (`tachyon/chronicles/<uuid>.json`), looks at its players' items once a
//! second, and gives the rewards a player claims; it tells each player its
//! progress (`tachyon:quests`). The player's window shows the book (the
//! quests key, J), asks for its progress when it opens
//! (`tachyon:quests_hello`) and claims (`tachyon:quest_claim`).

use super::super::{Game, Screen, commands::Target};
use horizon_client::{
    hud::{HudBatch, rgba},
    items::ItemStack,
    text::Text,
    ui,
};
use horizon_protocol::packets::{
    configuration::CustomPayload,
    play::{Clientbound, Serverbound},
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};
use winit::{event::MouseButton, keyboard::KeyCode};

/// The owner to a player: its progress.
pub(super) const PROGRESS: &str = "tachyon:quests";
/// A player to the owner: its progress, please.
const HELLO: &str = "tachyon:quests_hello";
/// A player to the owner: the rewards of this quest.
const CLAIM: &str = "tachyon:quest_claim";
/// Chronicles' row in Options > Mods, for its key.
pub(in crate::game) const ID: &str = "chronicles";
pub(in crate::game) const KEY: &str = "quests";
pub(in crate::game) const DEFAULT_KEY: &str = "key.keyboard.j";
/// How often the owner looks at its players' items.
const LOOK: Duration = Duration::from_secs(1);

#[derive(Deserialize)]
pub(super) struct Book {
    pub chapters: Vec<Chapter>,
}
#[derive(Deserialize)]
pub(super) struct Chapter {
    pub id: String,
    pub icon: String,
    pub quests: Vec<Quest>,
}
#[derive(Deserialize)]
pub(super) struct Quest {
    pub id: String,
    pub icon: String,
    /// The quests to finish first.
    #[serde(default)]
    pub after: Vec<String>,
    /// The items the player must hold at once.
    #[serde(default)]
    pub tasks: Vec<Stack>,
    #[serde(default)]
    pub rewards: Vec<Stack>,
}
#[derive(Clone, Deserialize)]
pub(super) struct Stack {
    pub item: String,
    #[serde(default = "one")]
    pub count: u32,
}
fn one() -> u32 {
    1
}

impl Book {
    fn quests(&self) -> impl Iterator<Item = &Quest> {
        self.chapters.iter().flat_map(|c| &c.quests)
    }
    fn quest(&self, id: &str) -> Option<&Quest> {
        self.quests().find(|q| q.id == id)
    }
}

/// A player's progress: the quests done, and those whose rewards it took.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Progress {
    pub done: BTreeSet<String>,
    pub claimed: BTreeSet<String>,
}
impl Progress {
    /// Whether the quests before `q` are done.
    pub fn open(&self, q: &Quest) -> bool {
        q.after.iter().all(|a| self.done.contains(a))
    }
    /// Marks done each open quest whose items the player holds (`held`:
    /// item → count); the ones it marked.
    pub fn update(&mut self, book: &Book, held: &HashMap<String, u32>) -> Vec<String> {
        let mut marked = Vec::new();
        // Again while it marks: a quest done opens those after it.
        loop {
            let ready: Vec<String> = book
                .quests()
                .filter(|q| !self.done.contains(&q.id) && self.open(q))
                .filter(|q| {
                    q.tasks
                        .iter()
                        .all(|t| held.get(&t.item).copied().unwrap_or(0) >= t.count)
                })
                .map(|q| q.id.clone())
                .collect();
            if ready.is_empty() {
                return marked;
            }
            for id in ready {
                self.done.insert(id.clone());
                marked.push(id);
            }
        }
    }
    /// Takes the rewards of a done quest not claimed yet.
    pub fn claim(&mut self, book: &Book, id: &str) -> Option<Vec<Stack>> {
        let q = book.quest(id)?;
        if !self.done.contains(id) || !self.claimed.insert(id.to_owned()) {
            return None;
        }
        Some(q.rewards.clone())
    }
}

/// The quests, read once.
pub(super) fn book() -> Option<&'static Book> {
    static BOOK: OnceLock<Option<Book>> = OnceLock::new();
    BOOK.get_or_init(|| {
        let path = horizon_client::brand::content()?
            .join("chronicles/data/chronicles/tachyon/quests.json");
        let text = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&text)
            .map_err(|e| log::warn!("Tachyon: {}: {e}", path.display()))
            .ok()
    })
    .as_ref()
}

/// Counts of the items in some slots.
fn held<'a>(slots: impl IntoIterator<Item = &'a Option<ItemStack>>) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    for s in slots.into_iter().flatten() {
        *out.entry(s.item.to_string()).or_insert(0) += s.count;
    }
    out
}

/// What a player asked of the owner.
enum Ask {
    Hello,
    Claim(String),
}

/// The owner's side: the players' progress in the world at `root`.
#[derive(Default)]
struct Owner {
    root: Option<PathBuf>,
    players: HashMap<u128, Progress>,
    asked: Vec<(u32, Ask)>,
    looked: Option<Instant>,
}
impl Owner {
    fn file(root: &Path, uuid: u128) -> PathBuf {
        root.join("tachyon")
            .join("chronicles")
            .join(format!("{uuid:032x}.json"))
    }
    fn progress(&mut self, uuid: u128) -> &mut Progress {
        let root = self.root.clone();
        self.players.entry(uuid).or_insert_with(|| {
            root.and_then(|r| std::fs::read_to_string(Self::file(&r, uuid)).ok())
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default()
        })
    }
    fn save(&self, uuid: u128) {
        let (Some(root), Some(p)) = (&self.root, self.players.get(&uuid)) else {
            return;
        };
        let path = Self::file(root, uuid);
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, serde_json::to_vec(p).unwrap_or_default()));
        if let Err(e) = written {
            log::warn!("Tachyon: {}: {e}", path.display());
        }
    }
}

/// The window's side: its progress, and the book's page.
#[derive(Default)]
struct Client {
    progress: Option<Progress>,
    /// The quest shown, by its place in the book.
    selected: usize,
    scroll: f32,
}

thread_local! {
    static OWNER: RefCell<Owner> = RefCell::new(Owner::default());
    static CLIENT: RefCell<Client> = RefCell::new(Client::default());
}

/// The book's layout in GUI pixels: the list on the left, the page on the
/// right, both under the title and over Done.
const LIST_W: f32 = 150.0;
const ROW: f32 = 20.0;
const TOP: f32 = 32.0;

impl Game<'_> {
    /// A player's payload to the owner (game/lan_actions.rs): kept for the
    /// next look, when the owner may act for the player.
    pub(in crate::game) fn tachyon_guest_payload(&mut self, conn: u32, channel: &str, data: &[u8]) {
        let ask = match channel {
            HELLO => Ask::Hello,
            CLAIM => match std::str::from_utf8(data) {
                Ok(id) => Ask::Claim(id.to_owned()),
                Err(_) => return,
            },
            _ => return,
        };
        OWNER.with_borrow_mut(|o| {
            if o.asked.len() < 64 {
                o.asked.push((conn, ask));
            }
        });
    }

    /// The owner's look at its players, once a second: quests done, rewards
    /// claimed, progress told.
    pub(super) fn tachyon_quests_tick(&mut self) {
        let Some(book) = book() else {
            return;
        };
        let Some(host) = self.lan.as_ref() else {
            return;
        };
        let now = Instant::now();
        let root = self.world_root.clone();
        let asked = OWNER.with_borrow_mut(|o| {
            if o.root.as_ref() != Some(&root) {
                *o = Owner {
                    root: Some(root.clone()),
                    ..Owner::default()
                };
            }
            if !o.asked.is_empty() || o.looked.is_none_or(|t| now - t >= LOOK) {
                o.looked = Some(now);
                Some(std::mem::take(&mut o.asked))
            } else {
                None
            }
        });
        let Some(asked) = asked else {
            return;
        };
        let players: Vec<(u32, u128, HashMap<String, u32>)> = host
            .hub
            .guests
            .iter()
            .map(|g| {
                let slots = g.inventory.iter().chain(&g.armor).chain([&g.offhand]);
                (g.conn, g.uuid, held(slots))
            })
            .collect();
        let uuid_of = |conn: u32| players.iter().find(|p| p.0 == conn).map(|p| p.1);
        let mut tell: Vec<u32> = Vec::new();
        let mut gifts: Vec<(u32, Vec<Stack>)> = Vec::new();
        OWNER.with_borrow_mut(|o| {
            for (conn, uuid, held) in &players {
                if !o.progress(*uuid).update(book, held).is_empty() {
                    o.save(*uuid);
                    tell.push(*conn);
                }
            }
            for (conn, ask) in asked {
                let Some(uuid) = uuid_of(conn) else {
                    continue;
                };
                if let Ask::Claim(id) = ask
                    && let Some(rewards) = o.progress(uuid).claim(book, &id)
                {
                    o.save(uuid);
                    gifts.push((conn, rewards));
                }
                tell.push(conn);
            }
        });
        for (conn, rewards) in gifts {
            self.for_player(Target::Guest(conn), |g| {
                for r in &rewards {
                    if g.res.items().get(&r.item).is_some() {
                        g.give_or_drop(ItemStack::new(&r.item, r.count.clamp(1, 64)));
                    }
                }
            });
        }
        tell.sort_unstable();
        tell.dedup();
        let Some(host) = self.lan.as_ref() else {
            return;
        };
        for conn in tell {
            let Some(uuid) = uuid_of(conn) else {
                continue;
            };
            let data = OWNER.with_borrow_mut(|o| serde_json::to_vec(o.progress(uuid)));
            if let Ok(data) = data {
                host.hub.send_to(
                    conn,
                    &Clientbound::CustomPayload(CustomPayload {
                        channel: PROGRESS.into(),
                        data,
                    }),
                );
            }
        }
    }

    /// The owner's word on this window's progress: quests newly done are
    /// told in chat.
    pub(super) fn tachyon_quests_payload(&mut self, data: &[u8]) {
        let Ok(progress) = serde_json::from_slice::<Progress>(data) else {
            log::warn!("Tachyon: a {PROGRESS} payload that doesn't read");
            return;
        };
        let before = CLIENT.with_borrow_mut(|c| c.progress.replace(progress.clone()));
        if let Some(before) = before {
            for id in progress.done.difference(&before.done) {
                let name = self.translate(&format!("chronicles.quest.{id}"), id);
                let line = self
                    .translate("chronicles.done", "Quest complete: %s")
                    .replace("%s", &name);
                self.native_chat(&line);
            }
        }
    }

    /// The quests key: opens the book while playing.
    pub(in crate::game) fn tachyon_quests_key(&mut self, key: &str, pressed: bool) {
        if !pressed || book().is_none() || !self.native_playing() || self.remote.is_none() {
            return;
        }
        if key != self.native_mod_key(ID, KEY, DEFAULT_KEY) {
            return;
        }
        self.tachyon_quests_send(HELLO, Vec::new());
        self.screen = Screen::Mod;
        self.capture(false);
    }

    fn tachyon_quests_send(&self, channel: &str, data: Vec<u8>) {
        self.send(Serverbound::CustomPayload(CustomPayload {
            channel: channel.into(),
            data,
        }));
    }

    /// The book (Screen::Mod in a build without WebAssembly mods): the
    /// quests down the left, the chosen one's page on the right.
    pub(in crate::game) fn tachyon_screen_draw(&mut self, batch: &mut HudBatch) {
        let Some(book) = book() else {
            self.tachyon_screen_close();
            return;
        };
        let (selected, scroll, progress) =
            CLIENT.with_borrow(|c| (c.selected, c.scroll, c.progress.clone()));
        let progress = progress.unwrap_or_default();
        let atlas = &self.gui.atlas;
        let (w, h) = (batch.width, batch.height);
        ui::menu_background(batch, atlas, 0.0, 0.0, w, h, self.player.is_some());
        let title = Text::literal(self.translate("chronicles.title", "Chronicles"));
        ui::centered_text(batch, atlas, &title, 0.0, 12.0, w, u32::MAX);
        let (left, right) = self.tachyon_book_columns(w);
        let bottom = h - 36.0;
        batch.fill(atlas, left, TOP, LIST_W, bottom - TOP, rgba(0, 0, 0, 120));
        // The list: chapter headers, then their quests.
        let mut icons: Vec<(String, f32, f32)> = Vec::new();
        let mut y = TOP + 2.0 - scroll;
        let mut index = 0;
        for chapter in &book.chapters {
            if y > TOP - ROW && y < bottom {
                let name = self.translate(&format!("chronicles.chapter.{}", chapter.id), &chapter.id);
                batch.text(atlas, &name, left + 4.0, y + 6.0, rgba(255, 210, 122, 255), true);
            }
            y += ROW;
            for q in &chapter.quests {
                if y >= TOP && y + ROW <= bottom {
                    if index == selected {
                        batch.fill(atlas, left + 1.0, y, LIST_W - 2.0, ROW, rgba(255, 255, 255, 50));
                    } else if ui::hovered(self.cursor, left, y, LIST_W, ROW) {
                        batch.fill(atlas, left + 1.0, y, LIST_W - 2.0, ROW, rgba(255, 255, 255, 24));
                    }
                    icons.push((q.icon.clone(), left + 4.0, y + 2.0));
                    let color = quest_color(&progress, q);
                    let name = self.translate(&format!("chronicles.quest.{}", q.id), &q.id);
                    batch.text(atlas, &name, left + 24.0, y + 6.0, color, true);
                }
                y += ROW;
                index += 1;
            }
        }
        // The page.
        if let Some(q) = book.quests().nth(selected) {
            let page_w = (w - right - 10.0).max(100.0);
            let name = self.translate(&format!("chronicles.quest.{}", q.id), &q.id);
            icons.push((q.icon.clone(), right, TOP));
            batch.text(atlas, &name, right + 22.0, TOP + 4.0, quest_color(&progress, q), true);
            let text = self.translate(&format!("chronicles.quest.{}.text", q.id), "");
            let mut y = TOP + 24.0;
            for line in self.tachyon_wrap(&text, page_w) {
                batch.text(atlas, &line, right, y, rgba(224, 224, 224, 255), true);
                y += 10.0;
            }
            y += 6.0;
            let have = held(
                self.inventory
                    .iter()
                    .chain(&self.armor)
                    .chain([&self.offhand]),
            );
            if !q.tasks.is_empty() {
                let label = self.translate("chronicles.tasks", "Hold at once:");
                batch.text(atlas, &label, right, y, u32::MAX, true);
                y += 12.0;
                for t in &q.tasks {
                    let n = have.get(&t.item).copied().unwrap_or(0).min(t.count);
                    let done = progress.done.contains(&q.id) || n >= t.count;
                    icons.push((t.item.clone(), right, y));
                    let color = if done {
                        rgba(110, 230, 110, 255)
                    } else {
                        u32::MAX
                    };
                    let count = format!("{} / {}", if done { t.count } else { n }, t.count);
                    batch.text(atlas, &count, right + 22.0, y + 5.0, color, true);
                    y += 18.0;
                }
            }
            if !q.rewards.is_empty() {
                y += 4.0;
                let label = self.translate("chronicles.rewards", "Rewards:");
                batch.text(atlas, &label, right, y, u32::MAX, true);
                y += 12.0;
                for (i, r) in q.rewards.iter().enumerate() {
                    let x = right + i as f32 * 40.0;
                    icons.push((r.item.clone(), x, y));
                    batch.text(atlas, &format!("×{}", r.count), x + 18.0, y + 9.0, u32::MAX, true);
                }
                y += 22.0;
            }
            // Claim, or why not.
            let state = if progress.claimed.contains(&q.id) {
                Err(self.translate("chronicles.claimed", "Claimed"))
            } else if progress.done.contains(&q.id) {
                Ok(())
            } else if !progress.open(q) {
                Err(self.translate("chronicles.locked", "Finish the quests before it first"))
            } else {
                Err(String::new())
            };
            match state {
                Ok(()) => {
                    let label = Text::literal(self.translate("chronicles.claim", "Claim"));
                    let hover = ui::hovered(self.cursor, right, y + 4.0, 100.0, 20.0);
                    ui::button(batch, atlas, right, y + 4.0, 100.0, 20.0, &label, true, hover);
                }
                Err(note) if !note.is_empty() => {
                    batch.text(atlas, &note, right, y + 10.0, rgba(160, 160, 160, 255), true);
                }
                Err(_) => {}
            }
        }
        let done = Text::literal(self.translate("gui.done", "Done"));
        let hover = ui::hovered(self.cursor, w / 2.0 - 100.0, h - 27.0, 200.0, 20.0);
        ui::button(batch, atlas, (w / 2.0).floor() - 100.0, h - 27.0, 200.0, 20.0, &done, true, hover);
        for (item, x, y) in icons {
            if self.res.items().get(&item).is_some() {
                self.draw_item(batch, &ItemStack::new(&item, 1), x, y);
            }
        }
    }

    /// A click on the book: a quest in the list, Claim, or Done.
    pub(in crate::game) fn tachyon_screen_click(&mut self, button: MouseButton) {
        if button != MouseButton::Left {
            return;
        }
        let Some(book) = book() else {
            return;
        };
        let (w, h) = self.gui_size();
        let cursor = self.cursor;
        if ui::hovered(cursor, (w / 2.0).floor() - 100.0, h - 27.0, 200.0, 20.0) {
            self.audio.play_button_click();
            self.tachyon_screen_close();
            return;
        }
        let (left, right) = self.tachyon_book_columns(w);
        let bottom = h - 36.0;
        let (selected, scroll, progress) =
            CLIENT.with_borrow(|c| (c.selected, c.scroll, c.progress.clone().unwrap_or_default()));
        if ui::hovered(cursor, left, TOP, LIST_W, bottom - TOP) {
            let mut y = TOP + 2.0 - scroll;
            let mut index = 0;
            for chapter in &book.chapters {
                y += ROW;
                for _ in &chapter.quests {
                    if cursor.1 >= y && cursor.1 < y + ROW {
                        self.audio.play_button_click();
                        CLIENT.with_borrow_mut(|c| c.selected = index);
                        return;
                    }
                    y += ROW;
                    index += 1;
                }
            }
            return;
        }
        // Claim sits under the page; its place depends on the page's text,
        // so a click on the page's lower part near the button claims.
        let Some(q) = book.quests().nth(selected) else {
            return;
        };
        if progress.done.contains(&q.id)
            && !progress.claimed.contains(&q.id)
            && cursor.0 >= right
            && cursor.0 < right + 100.0
            && self.tachyon_claim_button_y(q, w).is_some_and(|y| cursor.1 >= y && cursor.1 < y + 20.0)
        {
            self.audio.play_button_click();
            self.tachyon_quests_send(CLAIM, q.id.clone().into_bytes());
        }
    }

    /// Where the page's Claim button is, as `tachyon_screen_draw` lays the
    /// page out.
    fn tachyon_claim_button_y(&self, q: &Quest, w: f32) -> Option<f32> {
        let (_, right) = self.tachyon_book_columns(w);
        let page_w = (w - right - 10.0).max(100.0);
        let text = self.translate(&format!("chronicles.quest.{}.text", q.id), "");
        let mut y = TOP + 24.0 + self.tachyon_wrap(&text, page_w).len() as f32 * 10.0 + 6.0;
        if !q.tasks.is_empty() {
            y += 12.0 + q.tasks.len() as f32 * 18.0;
        }
        if !q.rewards.is_empty() {
            y += 4.0 + 12.0 + 22.0;
        }
        Some(y + 4.0)
    }

    /// The wheel scrolls the list.
    pub(in crate::game) fn tachyon_screen_scroll(&mut self, delta: f32) {
        let Some(book) = book() else {
            return;
        };
        let (_, h) = self.gui_size();
        let rows = book.chapters.len() + book.quests().count();
        let max = (rows as f32 * ROW - (h - 36.0 - TOP)).max(0.0);
        CLIENT.with_borrow_mut(|c| c.scroll = (c.scroll - delta * ROW).clamp(0.0, max));
    }

    /// Keys on the book: none of its own (Esc closes it as any screen; the
    /// press that opened it comes here too, so it may not close it).
    pub(in crate::game) fn tachyon_screen_key(&mut self, _key: KeyCode) -> bool {
        false
    }

    /// Back to the world.
    pub(in crate::game) fn tachyon_screen_close(&mut self) {
        if self.screen == Screen::Mod {
            self.screen = Screen::Playing;
            if self.player.is_some() {
                self.capture(true);
            }
        }
    }

    /// The list's left edge and the page's, centred on the screen.
    fn tachyon_book_columns(&self, w: f32) -> (f32, f32) {
        let width = (w - 40.0).min(460.0);
        let left = ((w - width) / 2.0).floor();
        (left, left + LIST_W + 12.0)
    }

    /// `text` in lines no wider than `width`, broken between words.
    fn tachyon_wrap(&self, text: &str, width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in text.split_whitespace() {
            let next = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && self.native_text_width(&next) > width {
                lines.push(std::mem::replace(&mut line, word.to_owned()));
            } else {
                line = next;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    }
}

/// A quest's name colour: green when claimed, gold when its rewards wait,
/// white while open, grey while locked.
fn quest_color(progress: &Progress, q: &Quest) -> u32 {
    if progress.claimed.contains(&q.id) {
        rgba(110, 230, 110, 255)
    } else if progress.done.contains(&q.id) {
        rgba(255, 210, 122, 255)
    } else if progress.open(q) {
        u32::MAX
    } else {
        rgba(140, 140, 140, 255)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> Book {
        serde_json::from_str(
            r#"{"chapters": [{"id": "c", "icon": "minecraft:stick", "quests": [
                {"id": "a", "icon": "minecraft:stick", "rewards": [{"item": "minecraft:bread", "count": 2}]},
                {"id": "b", "icon": "minecraft:stick", "after": ["a"],
                 "tasks": [{"item": "minecraft:stick", "count": 4}]},
                {"id": "c", "icon": "minecraft:stick", "after": ["b"]}
            ]}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn quests_open_in_order_and_are_done_with_the_items_held() {
        let book = book();
        let mut p = Progress::default();
        let none = HashMap::new();
        // "a" asks nothing, so it is done at once; "b" waits for sticks.
        assert_eq!(p.update(&book, &none), ["a"]);
        let three = HashMap::from([("minecraft:stick".to_owned(), 3)]);
        assert!(p.update(&book, &three).is_empty());
        // Four sticks: "b", and then "c" which it opened.
        let four = HashMap::from([("minecraft:stick".to_owned(), 4)]);
        assert_eq!(p.update(&book, &four), ["b", "c"]);
    }

    #[test]
    fn rewards_are_claimed_once_and_only_when_done() {
        let book = book();
        let mut p = Progress::default();
        assert!(p.claim(&book, "a").is_none());
        p.update(&book, &HashMap::new());
        let rewards = p.claim(&book, "a").unwrap();
        assert_eq!((rewards[0].item.as_str(), rewards[0].count), ("minecraft:bread", 2));
        assert!(p.claim(&book, "a").is_none());
        assert!(p.claim(&book, "nothing").is_none());
        let text = serde_json::to_vec(&p).unwrap();
        assert_eq!(serde_json::from_slice::<Progress>(&text).unwrap(), p);
    }
}
