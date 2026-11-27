//! The client's custom screen for the Clipboard (`ClipboardScreen`).
//! Displays parchment checklist pages with checkboxes, inline text editing, item icons, and multi-page navigation.

use std::sync::{Mutex, PoisonError};

use create_kinetics::rules::clipboard::{self as rules, Content, Entry};
use create_kinetics::rules::trains::station_screen::TextField;
use crate::compat::{
    ClientTickPostEvent, Flow, Hooks, Host, NetFromServerEvent, ScreenCharTypedPreEvent,
    ScreenClosingEvent, ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent,
    ScreenMouseScrolledPreEvent, ScreenRenderPostEvent, Text,
};

use super::create_gui::{
    self as gui, clean_wire, BtnStyle, UiRect, FRAME_BRASS, FRAME_PARCHMENT, PANEL_PARCHMENT,
    ROW_PARCHMENT, TEXT_DARK, TEXT_MUTED, TEXT_RED, WOOD_BOARD, WOOD_TRIM,
};

pub const OPEN_CHANNEL: &str = "create:clip_open";
pub const SAVE_CHANNEL: &str = "create:clip_save";

const TAG: &str = "clipboard";
const SEP: char = '\u{1f}';
const PAGE_SEP: char = '\u{1e}';
const ENTRY_SEP: char = '\u{1d}';
const FIELD_SEP: char = '\u{1c}';

const PANEL_W: i32 = 284;
const PANEL_H: i32 = 248;

struct Screen;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Screen)
}

// --- wire codecs ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveClipboard {
    pub content: Content,
    /// `0` = none, `1` = checked sound, `2` = erased sound.
    pub sound_delta: u8,
}

fn clean_clip_field(s: &str) -> String {
    clean_wire(s)
        .chars()
        .filter(|c| !matches!(*c, ENTRY_SEP | FIELD_SEP))
        .collect()
}

fn encode_content_wire(content: &Content) -> String {
    let pages_wire = content
        .pages
        .iter()
        .map(|page| {
            page.iter()
                .map(|entry| {
                    let (icon_id, icon_n) = match &entry.icon {
                        Some((id, n)) => (clean_clip_field(id), n.to_string()),
                        None => (String::new(), "0".to_owned()),
                    };
                    format!(
                        "{}{f}{}{f}{}{f}{}",
                        u8::from(entry.checked),
                        clean_clip_field(&entry.text),
                        icon_id,
                        icon_n,
                        f = FIELD_SEP,
                    )
                })
                .collect::<Vec<_>>()
                .join(&ENTRY_SEP.to_string())
        })
        .collect::<Vec<_>>()
        .join(&PAGE_SEP.to_string());
    format!("{}{SEP}{}{SEP}{pages_wire}", u8::from(content.read_only), content.page)
}

fn decode_content_wire(read_only_raw: &str, page_raw: &str, pages_raw: &str) -> Option<Content> {
    let read_only = read_only_raw == "1";
    let mut pages = Vec::new();
    if !pages_raw.is_empty() {
        for raw_page in pages_raw.split(PAGE_SEP) {
            let mut entries = Vec::new();
            for raw_entry in raw_page.split(ENTRY_SEP) {
                if raw_entry.is_empty() {
                    continue;
                }
                let mut parts = raw_entry.splitn(4, FIELD_SEP);
                let checked = parts.next()? == "1";
                let text = parts.next()?.chars().take(rules::MAX_TEXT).collect();
                let icon_id = parts.next()?;
                let icon_n: i32 = parts.next()?.parse().ok()?;
                let icon = (!icon_id.is_empty()).then(|| (icon_id.to_owned(), icon_n.max(1)));
                if entries.len() < rules::MAX_ENTRIES_PER_PAGE {
                    entries.push(Entry { checked, text, icon });
                }
            }
            pages.push(entries);
        }
    }
    if pages.is_empty() {
        pages.push(Vec::new());
    }
    let page = page_raw.parse::<usize>().unwrap_or(0).min(pages.len() - 1);
    Some(Content { pages, read_only, page })
}

pub fn encode_open(content: &Content) -> String {
    format!("clipO1{SEP}{}", encode_content_wire(content))
}

pub fn decode_open(text: &str) -> Option<Content> {
    let mut it = text.splitn(4, SEP);
    if it.next()? != "clipO1" {
        return None;
    }
    decode_content_wire(it.next()?, it.next()?, it.next()?)
}

pub fn encode_save(save: &SaveClipboard) -> String {
    format!("clipS1{SEP}{}{SEP}{}", save.sound_delta, encode_content_wire(&save.content))
}

pub fn decode_save(text: &str) -> Option<SaveClipboard> {
    let mut it = text.splitn(5, SEP);
    if it.next()? != "clipS1" {
        return None;
    }
    let sound_delta: u8 = it.next()?.parse().ok()?;
    let content = decode_content_wire(it.next()?, it.next()?, it.next()?)?;
    Some(SaveClipboard { content, sound_delta })
}

// --- open screen state ------------------------------------------------------------------------------------------

struct Open {
    content: Content,
    fields: Vec<TextField>,
    sound_delta: u8,
    size: [i32; 2],
    close: bool,
}

impl Open {
    fn from_content(mut content: Content) -> Self {
        if content.pages.is_empty() {
            content.pages.push(Vec::new());
        }
        content.page = content.page.min(content.pages.len() - 1);
        let mut open = Self {
            content,
            fields: Vec::new(),
            sound_delta: 0,
            size: [0, 0],
            close: false,
        };
        open.rebuild_fields();
        open
    }

    fn origin(&self) -> (i32, i32) {
        gui::screen_origin(self.size, PANEL_W, PANEL_H)
    }

    fn page_idx(&self) -> usize {
        self.content.page.min(self.content.pages.len().saturating_sub(1))
    }

    fn sync_fields_to_content(&mut self) {
        let p = self.page_idx();
        if let Some(entries) = self.content.pages.get_mut(p) {
            for (i, field) in self.fields.iter().enumerate() {
                if let Some(entry) = entries.get_mut(i) {
                    entry.text = field.value();
                }
            }
        }
    }

    fn rebuild_fields(&mut self) {
        let p = self.page_idx();
        self.fields = self
            .content
            .pages
            .get(p)
            .map(|entries| {
                entries
                    .iter()
                    .map(|e| {
                        let mut f = TextField::new(rules::MAX_TEXT);
                        f.set(&e.text);
                        f
                    })
                    .collect()
            })
            .unwrap_or_default();
    }

    fn focused_index(&self) -> Option<usize> {
        self.fields.iter().position(|f| f.focused)
    }

    fn unfocus_all(&mut self) {
        for f in &mut self.fields {
            f.focus(false);
        }
        self.sync_fields_to_content();
    }

    fn switch_page(&mut self, next_page: usize) {
        self.unfocus_all();
        if self.content.pages.is_empty() {
            self.content.pages.push(Vec::new());
        }
        self.content.page = next_page.min(self.content.pages.len() - 1);
        self.rebuild_fields();
    }

    fn add_line_and_focus(&mut self) {
        if self.content.read_only {
            return;
        }
        self.unfocus_all();
        let p = self.page_idx();
        if self.content.pages[p].len() >= rules::MAX_ENTRIES_PER_PAGE {
            return;
        }
        self.content.pages[p].push(Entry::default());
        self.rebuild_fields();
        if let Some(last) = self.fields.last_mut() {
            last.focus(true);
        }
    }

    fn save_and_close(&mut self) {
        self.unfocus_all();
        send(SaveClipboard {
            content: self.content.clone(),
            sound_delta: self.sound_delta,
        });
        self.close = true;
    }
}

static CLIENT: Mutex<Option<Open>> = Mutex::new(None);
static OUTBOX: Mutex<Vec<SaveClipboard>> = Mutex::new(Vec::new());

fn client() -> std::sync::MutexGuard<'static, Option<Open>> {
    CLIENT.lock().unwrap_or_else(PoisonError::into_inner)
}

fn send(msg: SaveClipboard) {
    OUTBOX.lock().unwrap_or_else(PoisonError::into_inner).push(msg);
}

struct RowLayout {
    check: UiRect,
    icon_at: Option<(i32, i32)>,
    field: UiRect,
    delete: UiRect,
    full_add: UiRect,
}

fn row_layout(left: i32, top: i32, idx: usize, has_icon: bool, read_only: bool) -> RowLayout {
    let y = top + 36 + idx as i32 * 21;
    let check = UiRect::new(left + 16, y, 20, 19);
    let icon_at = has_icon.then_some((left + 39, y + 2));
    let field_x = if has_icon { left + 60 } else { left + 39 };
    let field_right = if read_only { left + PANEL_W - 16 } else { left + PANEL_W - 40 };
    let field = UiRect::new(field_x, y, (field_right - field_x).max(40), 19);
    let delete = UiRect::new(left + PANEL_W - 36, y, 20, 19);
    let full_add = UiRect::new(left + 16, y, PANEL_W - 32, 19);
    RowLayout { check, icon_at, field, delete, full_add }
}

fn page_bar_rects(left: i32, top: i32) -> (UiRect, UiRect, UiRect, UiRect, UiRect) {
    let y = top + 190;
    (
        UiRect::new(left + 16, y, 22, 20),
        UiRect::new(left + 84, y, 22, 20),
        UiRect::new(left + 112, y, 50, 20),
        UiRect::new(left + 166, y, 50, 20),
        UiRect::new(left + 220, y, 48, 20),
    )
}

fn footer_rects(left: i32, top: i32) -> (UiRect, UiRect, UiRect) {
    let y = top + 214;
    (
        UiRect::new(left + 16, y, 54, 22),
        UiRect::new(left + 148, y, 56, 22),
        UiRect::new(left + 208, y, 60, 22),
    )
}

impl Hooks for Screen {
    fn net_from_server(&self, host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        if event.channel != OPEN_CHANNEL {
            return;
        }
        let Ok(text) = std::str::from_utf8(event.data) else { return };
        let Some(content) = decode_open(text) else { return };
        if gui::open(host, TAG) {
            *client() = Some(Open::from_content(content));
        }
    }

    fn client_tick_post(&self, host: &mut dyn Host, _: &mut ClientTickPostEvent) {
        if !host.window() {
            return;
        }
        let pending = std::mem::take(&mut *OUTBOX.lock().unwrap_or_else(PoisonError::into_inner));
        for msg in pending {
            let _ = host.net_send_to_server(SAVE_CHANNEL, encode_save(&msg).as_bytes());
        }
        let close_now = {
            let mut c = client();
            if c.is_some() && gui::sync_closed(&*host, TAG) {
                *c = None;
                return;
            }
            let Some(open) = c.as_mut() else { return };
            if open.close {
                *c = None;
                true
            } else {
                false
            }
        };
        if close_now {
            gui::close(host, TAG);
        }
    }

    fn screen_closing(&self, _: &dyn Host, event: &mut ScreenClosingEvent<'_>) {
        if event.screen.id == "mod" {
            *client() = None;
            gui::on_screen_closing(TAG);
        }
    }

    fn screen_render_post(&self, host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>) {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return };
        open.size = [event.batch.width as i32, event.batch.height as i32];
        draw(host, event, open);
    }

    fn screen_mouse_button_pressed_pre(&self, host: &dyn Host, event: &mut ScreenMouseButtonPressedPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        if open.size[0] == 0 {
            return Flow::Cancel;
        }
        let (left, top) = open.origin();
        let mouse = event.screen.mouse;
        let ro = open.content.read_only;
        let p = open.page_idx();
        let len = open.content.pages[p].len();

        for i in 0..len {
            let has_icon = open.content.pages[p][i].icon.is_some();
            let rl = row_layout(left, top, i, has_icon, ro);
            if rl.check.contains(mouse) {
                if !ro {
                    open.unfocus_all();
                    let now = !open.content.pages[p][i].checked;
                    open.content.pages[p][i].checked = now;
                    open.sound_delta = if now { 1 } else { 2 };
                }
                return Flow::Cancel;
            }
            if !ro && rl.field.contains(mouse) {
                let was_focused = open.fields.get(i).is_some_and(|f| f.focused);
                open.unfocus_all();
                if let Some(field) = open.fields.get_mut(i) {
                    field.focus(true);
                    if was_focused {
                        field.select_all();
                    }
                }
                return Flow::Cancel;
            }
            if !ro && rl.delete.contains(mouse) {
                open.unfocus_all();
                open.content.pages[p].remove(i);
                open.rebuild_fields();
                return Flow::Cancel;
            }
        }

        if !ro && len < rules::MAX_ENTRIES_PER_PAGE {
            let rl = row_layout(left, top, len, false, ro);
            if rl.full_add.contains(mouse) {
                open.add_line_and_focus();
                return Flow::Cancel;
            }
        }

        let (prev_btn, next_btn, add_line_btn, new_page_btn, rem_page_btn) = page_bar_rects(left, top);
        let page_count = open.content.pages.len();
        if prev_btn.contains(mouse) && page_count > 1 {
            open.switch_page(if p == 0 { page_count - 1 } else { p - 1 });
            return Flow::Cancel;
        }
        if next_btn.contains(mouse) && page_count > 1 {
            open.switch_page((p + 1) % page_count);
            return Flow::Cancel;
        }
        if !ro && add_line_btn.contains(mouse) && len < rules::MAX_ENTRIES_PER_PAGE {
            open.add_line_and_focus();
            return Flow::Cancel;
        }
        if !ro && new_page_btn.contains(mouse) {
            open.unfocus_all();
            open.content.pages.push(Vec::new());
            let new_idx = open.content.pages.len() - 1;
            open.switch_page(new_idx);
            return Flow::Cancel;
        }
        if !ro && rem_page_btn.contains(mouse) {
            open.unfocus_all();
            if open.content.pages.len() <= 1 {
                open.content.pages[0].clear();
                open.content.page = 0;
            } else {
                open.content.pages.remove(p);
                open.content.page = p.min(open.content.pages.len() - 1);
            }
            open.rebuild_fields();
            return Flow::Cancel;
        }

        let (clear_btn, close_btn, save_btn) = footer_rects(left, top);
        if !ro && clear_btn.contains(mouse) {
            open.unfocus_all();
            open.content = Content {
                pages: vec![Vec::new()],
                read_only: false,
                page: 0,
            };
            open.rebuild_fields();
            return Flow::Cancel;
        }
        if close_btn.contains(mouse) {
            open.close = true;
            return Flow::Cancel;
        }
        if save_btn.contains(mouse) {
            open.save_and_close();
            return Flow::Cancel;
        }

        open.unfocus_all();
        Flow::Cancel
    }

    fn screen_mouse_scrolled_pre(&self, host: &dyn Host, event: &mut ScreenMouseScrolledPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        let count = open.content.pages.len();
        if count > 1 {
            let p = open.page_idx();
            let next = if event.scroll[1] > 0.0 {
                if p == 0 { count - 1 } else { p - 1 }
            } else {
                (p + 1) % count
            };
            open.switch_page(next);
        }
        Flow::Cancel
    }

    fn screen_char_typed_pre(&self, host: &dyn Host, event: &mut ScreenCharTypedPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        if !open.content.read_only
            && let Some(idx) = open.focused_index()
            && let Some(field) = open.fields.get_mut(idx)
        {
            field.insert(event.c);
            open.sync_fields_to_content();
        }
        Flow::Cancel
    }

    fn screen_key_pressed_pre(&self, host: &dyn Host, event: &mut ScreenKeyPressedPreEvent<'_>) -> Flow {
        if event.screen.id != "mod" || !gui::is_active(host, TAG) {
            return Flow::Continue;
        }
        let mut c = client();
        let Some(open) = c.as_mut() else { return Flow::Continue };
        if let Some(idx) = open.focused_index() {
            if event.key == "key.keyboard.escape" || event.key == "key.keyboard.enter" || event.key == "key.keyboard.kp.enter" {
                open.unfocus_all();
                return Flow::Cancel;
            }
            if let Some(field) = open.fields.get_mut(idx) {
                field.key(event.key);
                open.sync_fields_to_content();
            }
            return Flow::Cancel;
        }
        if event.key == "key.keyboard.escape" {
            return Flow::Continue;
        }
        if event.key == "key.keyboard.enter" || event.key == "key.keyboard.kp.enter" {
            open.save_and_close();
            return Flow::Cancel;
        }
        Flow::Cancel
    }
}

fn draw(host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>, open: &Open) {
    gui::fill(host, event, UiRect::new(0, 0, open.size[0], open.size[1]), gui::DIM);
    let (left, top) = open.origin();

    // Wooden clipboard backing + brass clip + inner parchment sheet.
    gui::panel(host, event, UiRect::new(left, top + 6, PANEL_W, PANEL_H - 6), WOOD_TRIM, WOOD_BOARD);
    gui::panel(host, event, UiRect::new(left + 10, top + 14, PANEL_W - 20, PANEL_H - 22), FRAME_PARCHMENT, PANEL_PARCHMENT);
    gui::panel(host, event, UiRect::new(left + (PANEL_W - 96) / 2, top, 96, 12), WOOD_TRIM, FRAME_BRASS);

    let p = open.page_idx();
    let total = open.content.pages.len();
    let ro = open.content.read_only;
    host.text(event.batch, "Clipboard", [(left + 16) as f32, (top + 20) as f32], TEXT_DARK, false);
    if ro {
        host.text(event.batch, "READ ONLY", [(left + 84) as f32, (top + 20) as f32], TEXT_RED, false);
    }
    let page_title = format!("Page {}/{}", p + 1, total);
    let pw = host.text_width(&page_title);
    host.text(event.batch, &page_title, [(left + PANEL_W - 16) as f32 - pw, (top + 20) as f32], TEXT_MUTED, false);

    let mut tip: Option<Vec<String>> = None;
    let entries = &open.content.pages[p];
    for (i, entry) in entries.iter().enumerate() {
        let rl = row_layout(left, top, i, entry.icon.is_some(), ro);
        let check_style = if entry.checked { BtnStyle::Primary } else { BtnStyle::Parchment };
        let check_lbl = if entry.checked { "v" } else { " " };
        gui::button(host, event, rl.check, check_lbl, check_style, !ro);

        if let Some((ix, iy)) = rl.icon_at
            && let Some((icon_id, count)) = &entry.icon
        {
            host.item(event.batch, icon_id, [ix as f32, iy as f32], 14.0);
            if *count > 1 {
                host.text(event.batch, &count.to_string(), [(ix + 11) as f32, (iy + 7) as f32], TEXT_DARK, false);
            }
        }

        if let Some(field) = open.fields.get(i) {
            gui::text_field(host, event, rl.field, field, "Type note...", false);
        }
        if !ro && gui::button(host, event, rl.delete, "x", BtnStyle::Danger, true) {
            tip = Some(vec!["Remove entry".to_owned()]);
        }
    }

    if !ro && entries.len() < rules::MAX_ENTRIES_PER_PAGE {
        let rl = row_layout(left, top, entries.len(), false, ro);
        gui::fill(host, event, rl.full_add, FRAME_PARCHMENT);
        gui::fill(host, event, rl.full_add.inset(1), ROW_PARCHMENT);
        let lbl = "+ Tap to add checklist entry...";
        host.text(event.batch, lbl, [(rl.full_add.x + 6) as f32, (rl.full_add.y + 6) as f32], 0x6E604A, false);
    }

    let (prev_btn, next_btn, add_line_btn, new_page_btn, rem_page_btn) = page_bar_rects(left, top);
    gui::button(host, event, prev_btn, "<", BtnStyle::Parchment, total > 1);
    let mid_lbl = format!("{}/{}", p + 1, total);
    let mw = host.text_width(&mid_lbl);
    host.text(event.batch, &mid_lbl, [(left + 61) as f32 - mw / 2.0, (top + 196) as f32], TEXT_DARK, false);
    gui::button(host, event, next_btn, ">", BtnStyle::Parchment, total > 1);
    gui::button(host, event, add_line_btn, "+Line", BtnStyle::Parchment, !ro && entries.len() < rules::MAX_ENTRIES_PER_PAGE);
    gui::button(host, event, new_page_btn, "+Page", BtnStyle::Parchment, !ro);
    gui::button(host, event, rem_page_btn, "-Page", BtnStyle::Parchment, !ro);

    let (clear_btn, close_btn, save_btn) = footer_rects(left, top);
    gui::button(host, event, clear_btn, "Clear", BtnStyle::Danger, !ro);
    gui::button(host, event, close_btn, "Close", BtnStyle::Normal, true);
    gui::button(host, event, save_btn, "Save", BtnStyle::Primary, true);

    if let Some(lines) = tip {
        let lines: Vec<Text> = lines.into_iter().map(Text::literal).collect();
        host.gui_tooltip(event.batch, &lines, event.screen.mouse);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_save_clipboard_round_trip_with_pages_and_icons() {
        let content = Content {
            pages: vec![
                vec![
                    Entry { checked: true, text: "Brass Casing".into(), icon: Some(("create:brass_casing".into(), 12)) },
                    Entry { checked: false, text: "Shafts".into(), icon: None },
                ],
                vec![],
            ],
            read_only: false,
            page: 1,
        };
        assert_eq!(decode_open(&encode_open(&content)), Some(content.clone()));

        let save = SaveClipboard { content, sound_delta: 2 };
        assert_eq!(decode_save(&encode_save(&save)), Some(save));
    }

    #[test]
    fn open_screen_handles_page_switch_and_add_line() {
        let mut open = Open::from_content(Content::default());
        open.add_line_and_focus();
        assert_eq!(open.content.pages[0].len(), 1);
        assert_eq!(open.focused_index(), Some(0));
        open.fields[0].insert('A');
        open.sync_fields_to_content();
        assert_eq!(open.content.pages[0][0].text, "A");
    }
}
