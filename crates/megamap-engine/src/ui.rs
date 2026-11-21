//! The world map screen as a state machine: layout, hit testing and input -> `Action`. The glue draws from the
//! same layout functions, so what is clicked is what is drawn, and the tests drive it without a game.

use super::color;
use super::entities::Category;
use super::settings::{self, Row, CATEGORIES, INFO_FIRST};
use super::waypoints::{Waypoint, Waypoints, ICONS};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.x && p[0] < self.x + self.w && p[1] >= self.y && p[1] < self.y + self.h
    }
    pub fn arr(&self) -> [f32; 4] {
        [self.x, self.y, self.w, self.h]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    ZoomIn,
    ZoomOut,
    Center,
    List,
    Cave,
    Trail,
    Clouds,
    Relief,
    Grid,
    Settings,
    Entities,
    Dimension,
}

/// Toolbar: the button, its fallback label (drawn when the icon sprite is missing), the tooltip, and the icon
/// sprite tools/gen_art.py makes for it.
pub const BUTTONS: [(Button, &str, &str); 12] = [
    (Button::ZoomIn, "+", "megamap.tip.zoom_in"),
    (Button::ZoomOut, "-", "megamap.tip.zoom_out"),
    (Button::Center, "P", "megamap.tip.center"),
    (Button::List, "W", "megamap.tip.list"),
    (Button::Cave, "C", "megamap.tip.cave"),
    (Button::Trail, "T", "megamap.tip.trail"),
    (Button::Clouds, "F", "megamap.tip.clouds"),
    (Button::Relief, "R", "megamap.tip.relief"),
    (Button::Grid, "G", "megamap.tip.grid"),
    (Button::Settings, "S", "megamap.tip.settings"),
    (Button::Entities, "E", "megamap.tip.entities"),
    (Button::Dimension, "D", "megamap.tip.dimension"),
];

/// Icon sprite per toolbar button, same order as `BUTTONS`.
pub const BUTTON_ICONS: [&str; 12] = [
    "megamap:gui/tb_zoom_in",
    "megamap:gui/tb_zoom_out",
    "megamap:gui/tb_center",
    "megamap:gui/tb_list",
    "megamap:gui/tb_cave",
    "megamap:gui/tb_trail",
    "megamap:gui/tb_clouds",
    "megamap:gui/tb_relief",
    "megamap:gui/tb_grid",
    "megamap:gui/tb_settings",
    "megamap:gui/tb_entities",
    "megamap:gui/tb_dimension",
];

/// An entry of the right-click menu: over the map, or over a waypoint (on the map or in the list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    WpHere,
    CoordsChat,
    SharePos,
    Teleport,
    Center,
    WpEdit,
    WpToggle,
    WpCenter,
    WpShare,
    WpTeleport,
    WpDelete,
}

pub const MENU_MAP: [MenuItem; 5] = [MenuItem::WpHere, MenuItem::CoordsChat, MenuItem::SharePos, MenuItem::Teleport, MenuItem::Center];
pub const MENU_WAYPOINT: [MenuItem; 6] = [MenuItem::WpEdit, MenuItem::WpToggle, MenuItem::WpCenter, MenuItem::WpShare, MenuItem::WpTeleport, MenuItem::WpDelete];

impl MenuItem {
    /// The lang key (`visible`: the waypoint's state, for the show / hide entry).
    pub fn key(self, visible: bool) -> &'static str {
        match self {
            MenuItem::WpHere => "megamap.menu.wp_here",
            MenuItem::CoordsChat => "megamap.menu.coords_chat",
            MenuItem::SharePos => "megamap.menu.share_pos",
            MenuItem::Teleport => "megamap.menu.tp",
            MenuItem::Center => "megamap.menu.center",
            MenuItem::WpEdit => "megamap.menu.wp_edit",
            MenuItem::WpToggle if visible => "megamap.menu.wp_hide",
            MenuItem::WpToggle => "megamap.menu.wp_show",
            MenuItem::WpCenter => "megamap.menu.wp_center",
            MenuItem::WpShare => "megamap.menu.wp_share",
            MenuItem::WpTeleport => "megamap.menu.wp_tp",
            MenuItem::WpDelete => "megamap.menu.wp_delete",
        }
    }
}

/// How near (GUI px) a right click must be to a waypoint's mark to open its menu.
pub const WP_PICK_PX: f32 = 8.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    /// Leave the screen.
    Close,
    /// Prefill the chat box with this text (the API has no clipboard; the player copies or sends it).
    ChatPrefill(String),
    SaveWaypoint(Option<usize>, Waypoint),
    DeleteWaypoint(usize),
    ShareWaypoint(usize),
    ToggleCave,
    ToggleTrail,
    ToggleClouds,
    CycleRelief,
    CycleGrid,
    /// A settings item by its flat index (`Settings::click`).
    SettingClick(usize),
    /// A click on a slider item's track at fraction 0..1 (`settings::slider_step`).
    SettingSlider(usize, f32),
    /// Move the info line at this position up (`true`) or down.
    InfoMove(usize, bool),
    /// Change one part of an entity kind's look.
    EntityEdit(String, EntityField),
    /// Show this dimension folder's map (`None`: the player's own).
    ViewDimension(Option<String>),
    /// The settings category shown changed (remember it: `Settings::ui_cat`).
    SettingsCat(usize),
    /// "Reset category" in the settings.
    ResetCategory(usize),
}

/// The preview's weather switch (preview only; the game's weather is not touched).
pub const PREVIEW_WEATHERS: [&str; 7] = [
    "megamap.preview.clear",
    "megamap.preview.rain",
    "megamap.preview.storm",
    "megamap.preview.sunset",
    "megamap.preview.night",
    "megamap.preview.nether",
    "megamap.preview.end",
];
/// `t` shortened with ".." until `width` of it fits `max` (GUI pixels); true when it was shortened.
pub fn fit(t: &str, max: f32, width: &dyn Fn(&str) -> f32) -> (String, bool) {
    if width(t) <= max {
        return (t.to_string(), false);
    }
    let mut s = t.to_string();
    loop {
        s.pop();
        let cut = s.trim_end();
        if cut.is_empty() {
            return (String::new(), true);
        }
        let c = format!("{cut}..");
        if width(&c) <= max {
            return (c, true);
        }
    }
}

/// A settings row's text laid out in `avail` pixels: the name left, the value right-aligned. The value keeps
/// its full text while it takes at most 60% of the row; the name gives way first. `cut` is true when either
/// was shortened (the row then shows its full text in a tooltip).
#[derive(Clone, Debug, PartialEq)]
pub struct RowText {
    pub name: String,
    pub value: String,
    pub cut: bool,
}

/// Gap between a row's name and its value, GUI pixels.
pub const ROW_GAP: f32 = 6.0;

pub fn row_text(name: &str, value: &str, avail: f32, width: &dyn Fn(&str) -> f32) -> RowText {
    if value.is_empty() {
        let (name, cut) = fit(name, avail, width);
        return RowText { name, value: String::new(), cut };
    }
    let (value, vcut) = fit(value, (avail * 0.6).max(avail - width(name) - ROW_GAP), width);
    let (name, ncut) = fit(name, avail - width(&value) - ROW_GAP, width);
    RowText { name, value, cut: vcut || ncut }
}

/// Settings window layout.
pub const NAV_W: f32 = 96.0;
pub const NAV_ROW: f32 = 18.0;
/// The settings window's width when the screen has room (narrower screens get what is left).
pub const SETTINGS_W: f32 = 540.0;
pub const PREVIEW_W: f32 = 124.0;
/// The preview image (square, GUI pixels).
pub const PREVIEW_IMG: f32 = 116.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityField {
    Mini,
    Map,
    Color,
    Icon,
    Size,
    Reset,
}

/// The side windows of the map: settings (two tabs), entities, dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    Settings,
    Entities,
    Dimensions,
}

/// Rows of the entity detail view, in order.
pub const ENTITY_FIELDS: [EntityField; 6] =
    [EntityField::Mini, EntityField::Map, EntityField::Color, EntityField::Icon, EntityField::Size, EntityField::Reset];

#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub kind: PanelKind,
    /// Settings tab (0 minimap, 1 map), or the entity category filter (0 all, then `Category::ALL`).
    pub tab: usize,
    pub scroll: usize,
    pub search: String,
    pub search_focus: bool,
    /// The entity kind whose detail view is open.
    pub selected: Option<String>,
    /// Settings: the category shown (index into `settings::CATEGORIES`).
    pub cat: usize,
    /// Settings: the preview's weather (`PREVIEW_WEATHERS`).
    pub preview_weather: usize,
}

impl Panel {
    pub fn new(kind: PanelKind) -> Self {
        Panel {
            kind,
            tab: 0,
            scroll: 0,
            search: String::new(),
            search_focus: kind == PanelKind::Entities,
            selected: None,
            cat: 0,
            preview_weather: 0,
        }
    }
    /// The entity category filter.
    pub fn category(&self) -> Option<Category> {
        self.tab.checked_sub(1).and_then(|i| Category::ALL.get(i).copied())
    }
    /// Number of header tabs.
    pub fn tabs(&self) -> usize {
        match self.kind {
            PanelKind::Settings => 2,
            PanelKind::Entities if self.selected.is_none() => 1 + Category::ALL.len(),
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub index: Option<usize>,
    pub draft: Waypoint,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Menu {
    pub at: [f32; 2],
    pub world: [i32; 2],
    /// The waypoint the menu is for (its entries then, under its name as a header); `None`: the map's.
    pub wp: Option<usize>,
}

impl Menu {
    pub fn items(&self) -> &'static [MenuItem] {
        if self.wp.is_some() { &MENU_WAYPOINT } else { &MENU_MAP }
    }
    /// Rows above the entries (the waypoint's name).
    pub fn header_rows(&self) -> usize {
        self.wp.is_some() as usize
    }
}

pub const EDIT_BUTTONS: [&str; 4] = ["megamap.edit.save", "megamap.edit.delete", "megamap.edit.share", "megamap.edit.cancel"];
pub const ROW: f32 = 12.0;
pub const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);
/// Width of the row labels in the waypoint dialog (the swatches start after it).
pub const DIALOG_LABEL_W: f32 = 62.0;
/// Side of the entities window's 3D preview, GUI pixels.
pub const ENT_PREVIEW_SIDE: f32 = 96.0;
/// The entity preview's zoom range (the wheel over it).
pub const SPIN_ZOOM_MIN: f32 = 0.6;
pub const SPIN_ZOOM_MAX: f32 = 4.0;
pub const MIN_BPP: f64 = 0.125;
pub const MAX_BPP: f64 = 64.0;

pub struct MapUi {
    /// The screen rect in GUI pixels, set every frame from the screen view.
    pub screen: Rect,
    pub center: [f64; 2],
    /// Blocks per GUI pixel.
    pub bpp: f64,
    pub cursor: [f32; 2],
    pub list_open: bool,
    pub search: String,
    pub scroll: usize,
    pub menu: Option<Menu>,
    pub edit: Option<Edit>,
    /// The search box has focus (typing filters) instead of nothing.
    pub search_focus: bool,
    /// Current dimension and player position, for new waypoints and distance sort.
    pub dimension: String,
    pub player: [f64; 3],
    pub dragging: bool,
    /// The entities window's 3D preview is being turned with the mouse.
    pub spin_drag: bool,
    /// The preview's turn from the mouse (degrees, added to the auto spin) and its pitch offset.
    pub spin_yaw: f32,
    pub spin_pitch: f32,
    /// The entity preview's magnification (the wheel over it), 1 = the whole model at any angle.
    pub spin_zoom: f32,
    /// Bumped on every change that alters the map image (pan, zoom).
    pub view_generation: u64,
    pub panel: Option<Panel>,
    /// The open panel's rows, kept current by the glue: entity kinds (entities), dimension folders
    /// (dimensions, the player's own first).
    pub panel_rows: Vec<String>,
    /// The settings content rows (`settings::rows`), refreshed by the glue with `refresh_settings_rows`.
    pub settings_rows: Vec<Row>,
    /// Headers of the settings groups folded away (a click on a header folds and unfolds its group).
    pub collapsed: Vec<&'static str>,
    /// The waypoint row clicked last and when: a second click within `DOUBLE_CLICK` opens its editor.
    pub last_row_click: Option<(usize, std::time::Instant)>,
    /// The settings category to open with (remembered across openings, `Settings::ui_cat`).
    pub settings_cat: usize,
    /// The translator the panels draw with (set by the glue from the language setting).
    pub tr: super::lang::Tr,
    /// The base name of new waypoints in the map's language.
    pub wp_name: String,
}

impl MapUi {
    pub fn new() -> Self {
        MapUi {
            screen: Rect::new(0.0, 0.0, 480.0, 270.0),
            center: [0.0, 0.0],
            bpp: 1.0,
            cursor: [0.0, 0.0],
            list_open: false,
            search: String::new(),
            scroll: 0,
            menu: None,
            edit: None,
            search_focus: false,
            dimension: "minecraft:overworld".into(),
            player: [0.0; 3],
            dragging: false,
            spin_drag: false,
            spin_yaw: 0.0,
            spin_pitch: 0.0,
            spin_zoom: 1.0,
            view_generation: 1,
            panel: None,
            panel_rows: Vec::new(),
            settings_rows: Vec::new(),
            collapsed: Vec::new(),
            last_row_click: None,
            settings_cat: 0,
            tr: super::lang::Tr::new(super::lang::Lang::En),
            wp_name: "Waypoint".into(), // lang-ok: replaced from the lang table by the glue
        }
    }

    /// Rebuilds the settings content rows from the current labels (search matches labels).
    pub fn refresh_settings_rows(&mut self, labels: &[String]) {
        let mut rows = std::mem::take(&mut self.settings_rows);
        match self.panel.as_ref().filter(|p| p.kind == PanelKind::Settings) {
            Some(p) => settings::rows(p.cat, &p.search, labels, &self.tr, &self.collapsed, &mut rows),
            None => rows.clear(),
        }
        self.settings_rows = rows;
    }

    pub fn is_settings(&self) -> bool {
        self.panel.as_ref().is_some_and(|p| p.kind == PanelKind::Settings)
    }

    /// Opened from the key: centred on the player, panels closed (the list open when `list`).
    pub fn open(&mut self, player: [f64; 3], dimension: &str, list: bool) {
        self.center = [player[0], player[2]];
        self.player = player;
        self.dimension = dimension.into();
        self.list_open = list;
        self.search_focus = list;
        self.menu = None;
        self.edit = None;
        self.panel = None;
        self.dragging = false;
        self.spin_drag = false;
        self.view_generation += 1;
    }

    /// Opens a panel, or closes it when it is the one open.
    pub fn toggle_panel(&mut self, kind: PanelKind) {
        if self.panel.as_ref().is_some_and(|p| p.kind == kind) {
            self.panel = None;
        } else {
            let mut p = Panel::new(kind);
            if kind == PanelKind::Settings {
                p.cat = self.settings_cat.min(CATEGORIES.len() - 1);
                p.tab = CATEGORIES[p.cat].tab;
            }
            self.panel = Some(p);
            self.panel_rows.clear();
            self.settings_rows.clear();
            self.list_open = false;
            self.search_focus = false;
        }
    }

    // ---- panel layout ----

    pub fn panel_rect(&self) -> Rect {
        let (want, hmax): (f32, f32) = if self.is_settings() { (SETTINGS_W, 300.0) } else { (250.0, 260.0) };
        let w = want.min(self.screen.w - 40.0).max(120.0);
        let h = (self.screen.h - 40.0).clamp(80.0, hmax);
        Rect::new(self.screen.x + self.screen.w - 28.0 - w, self.screen.y + 18.0, w, h)
    }
    pub fn panel_close_rect(&self) -> Rect {
        let r = self.panel_rect();
        Rect::new(r.x + r.w - 13.0, r.y + 2.0, 11.0, 11.0)
    }
    /// Header tab `i` (settings: the two tabs; entities: All and the categories).
    pub fn panel_tab_rect(&self, i: usize) -> Rect {
        let r = self.panel_rect();
        let n = self.panel.as_ref().map_or(1, |p| p.tabs()).max(1);
        let w = (r.w - 18.0) / n as f32;
        Rect::new(r.x + 2.0 + i as f32 * w, r.y + 2.0, w - 1.0, 12.0)
    }
    pub fn panel_search_rect(&self) -> Option<Rect> {
        let p = self.panel.as_ref()?;
        let r = self.panel_rect();
        match p.kind {
            PanelKind::Entities if p.selected.is_none() => Some(Rect::new(r.x + 2.0, r.y + 16.0, r.w - 4.0, 12.0)),
            PanelKind::Settings => {
                let right = self.settings_preview_rect().map_or(r.x + r.w - 2.0, |q| q.x - 3.0);
                Some(Rect::new(r.x + 2.0, r.y + 16.0, right - r.x - 2.0, 12.0))
            }
            _ => None,
        }
    }
    fn panel_top(&self) -> f32 {
        let r = self.panel_rect();
        r.y + if self.panel_search_rect().is_some() { 30.0 } else { 16.0 }
    }
    /// Settings: the bottom line that shows the hovered setting's hint.
    pub fn settings_hint_rect(&self) -> Option<Rect> {
        if !self.is_settings() {
            return None;
        }
        let r = self.panel_rect();
        let right = self.settings_preview_rect().map_or(r.x + r.w - 2.0, |q| q.x - 3.0);
        Some(Rect::new(r.x + 2.0, r.y + r.h - 14.0, right - r.x - 2.0, 12.0))
    }
    /// Settings: the right column with the live preview (only when the window is wide enough).
    pub fn settings_preview_rect(&self) -> Option<Rect> {
        if !self.is_settings() {
            return None;
        }
        let r = self.panel_rect();
        (r.w >= 380.0).then(|| Rect::new(r.x + r.w - 2.0 - PREVIEW_W, r.y + 16.0, PREVIEW_W, r.h - 18.0))
    }
    /// The preview image inside the preview column.
    pub fn settings_preview_image(&self) -> Option<Rect> {
        let c = self.settings_preview_rect()?;
        Some(Rect::new(c.x + (c.w - PREVIEW_IMG) / 2.0, c.y + 2.0, PREVIEW_IMG, PREVIEW_IMG))
    }
    /// The preview weather button `k` (two columns under the image).
    pub fn settings_weather_rect(&self, k: usize) -> Option<Rect> {
        let c = self.settings_preview_rect()?;
        let w = (c.w - 6.0) / 2.0;
        let y = c.y + PREVIEW_IMG + 6.0 + (k / 2) as f32 * 13.0;
        (y + 12.0 <= c.y + c.h).then(|| Rect::new(c.x + 2.0 + (k % 2) as f32 * (w + 2.0), y, w, 12.0))
    }
    /// Settings: the left navigation entry `k` of the current tab.
    pub fn settings_nav_rect(&self, k: usize) -> Option<Rect> {
        if !self.is_settings() {
            return None;
        }
        let r = self.panel_rect();
        let nav_w = if r.w >= 500.0 { NAV_W + 16.0 } else { NAV_W.min(r.w * 0.3) };
        Some(Rect::new(r.x + 2.0, self.panel_top() + k as f32 * NAV_ROW, nav_w, NAV_ROW - 1.0))
    }
    /// The global category index of the settings nav entry `k`.
    pub fn settings_nav_cat(&self, k: usize) -> Option<usize> {
        let p = self.panel.as_ref()?;
        settings::tab_categories(p.tab).nth(k)
    }
    /// The settings item under the cursor (content row), for the hint and the preview highlight.
    pub fn settings_hover(&self) -> Option<usize> {
        let p = self.panel.as_ref().filter(|p| p.kind == PanelKind::Settings)?;
        let slot = (0..self.panel_slots()).find(|&s| self.panel_row_rect(s).contains(self.cursor))?;
        match self.settings_rows.get(p.scroll + slot)? {
            Row::Item(i) => Some(*i),
            _ => None,
        }
    }
    /// Rows that fit in the panel.
    pub fn panel_slots(&self) -> usize {
        let r = self.panel_rect();
        let bottom = self.settings_hint_rect().map_or(r.y + r.h - 2.0, |h| h.y - 1.0);
        ((bottom - self.panel_top()) / ROW).max(1.0) as usize
    }
    pub fn panel_row_rect(&self, slot: usize) -> Rect {
        let r = self.panel_rect();
        let y = self.panel_top() + slot as f32 * ROW;
        if self.is_settings() {
            let x = self.settings_nav_rect(0).map_or(r.x + 2.0, |n| n.x + n.w + 3.0);
            let right = self.settings_preview_rect().map_or(r.x + r.w - 2.0, |q| q.x - 3.0);
            return Rect::new(x, y, right - x, ROW);
        }
        Rect::new(r.x + 2.0, y, r.w - 4.0, ROW)
    }
    /// The small boxes at a row's right end: entities `0` minimap / `1` map toggles, info lines `0` up / `1` down.
    /// The track of a slider item in row `slot` (the right part of the row).
    pub fn panel_slider_rect(&self, slot: usize) -> Rect {
        let r = self.panel_row_rect(slot);
        let w = (r.w * 0.38).clamp(40.0, 90.0);
        Rect::new(r.x + r.w - w - 3.0, r.y + 2.0, w, ROW - 4.0)
    }
    pub fn panel_row_box(&self, slot: usize, k: usize) -> Rect {
        let r = self.panel_row_rect(slot);
        Rect::new(r.x + r.w - 24.0 + k as f32 * 12.0, r.y + 1.0, 11.0, ROW - 2.0)
    }
    /// The entities window's 3D preview square (under the detail rows), when a kind is selected and it fits.
    pub fn entity_preview_rect(&self) -> Option<Rect> {
        self.panel.as_ref().filter(|p| p.kind == PanelKind::Entities && p.selected.is_some())?;
        let r = self.panel_rect();
        let top = self.panel_row_rect(ENTITY_FIELDS.len() + 1).y + 2.0;
        let side = ENT_PREVIEW_SIDE;
        if r.y + r.h - 14.0 - top < side {
            return None;
        }
        Some(Rect::new((r.x + (r.w - side) / 2.0).round(), top.round(), side, side))
    }
    /// Number of rows the open panel has.
    pub fn panel_len(&self) -> usize {
        match &self.panel {
            None => 0,
            Some(p) => match p.kind {
                PanelKind::Settings => self.settings_rows.len(),
                PanelKind::Entities if p.selected.is_some() => 1 + ENTITY_FIELDS.len(),
                _ => self.panel_rows.len(),
            },
        }
    }

    fn panel_press(&mut self, p: [f32; 2], button: i32) -> Action {
        if button != 0 {
            return Action::None;
        }
        if self.panel_close_rect().contains(p) {
            self.panel = None;
            return Action::None;
        }
        let search = self.panel_search_rect();
        let tabs = self.panel.as_ref().map_or(0, |q| q.tabs());
        let tab_hit = (0..tabs).find(|&i| self.panel_tab_rect(i).contains(p));
        let slots = self.panel_slots();
        let hit = (0..slots).find(|&s| self.panel_row_rect(s).contains(p));
        let boxes = hit.map(|s| (0..2).find(|&k| self.panel_row_box(s, k).contains(p)));
        let slider_f = hit.map(|s| self.panel_slider_rect(s)).filter(|r| r.contains(p)).map(|r| (p[0] - r.x) / r.w.max(1.0));
        let len = self.panel_len();
        let nav_hit = (0..CATEGORIES.len())
            .find(|&k| self.settings_nav_rect(k).is_some_and(|r| r.contains(p)))
            .and_then(|k| self.settings_nav_cat(k));
        let weather_hit = (0..PREVIEW_WEATHERS.len()).find(|&k| self.settings_weather_rect(k).is_some_and(|r| r.contains(p)));
        if self.entity_preview_rect().is_some_and(|r| r.contains(p)) {
            self.spin_drag = true;
            return Action::None;
        }
        let Some(panel) = self.panel.as_mut() else { return Action::None };
        panel.search_focus = search.is_some_and(|r| r.contains(p));
        if let Some(t) = tab_hit {
            if panel.tab != t {
                panel.tab = t;
                panel.scroll = 0;
                if panel.kind == PanelKind::Settings {
                    panel.cat = settings::tab_categories(t).next().unwrap_or(0);
                    panel.search.clear();
                    self.settings_cat = panel.cat;
                    return Action::SettingsCat(panel.cat);
                }
            }
            return Action::None;
        }
        if panel.kind == PanelKind::Settings {
            if let Some(c) = nav_hit {
                panel.cat = c;
                panel.scroll = 0;
                panel.search.clear();
                self.settings_cat = c;
                return Action::SettingsCat(c);
            }
            if let Some(k) = weather_hit {
                panel.preview_weather = k;
                return Action::None;
            }
        }
        let Some(slot) = hit else { return Action::None };
        let i = panel.scroll + slot;
        if i >= len {
            return Action::None;
        }
        let rbox = boxes.flatten();
        match panel.kind {
            PanelKind::Settings => match self.settings_rows[i] {
                Row::Header(h) => {
                    match self.collapsed.iter().position(|c| *c == h) {
                        Some(k) => {
                            self.collapsed.remove(k);
                        }
                        None => self.collapsed.push(h),
                    }
                    // The list got shorter: the scroll stays inside it once the rows are rebuilt.
                    if let Some(q) = self.panel.as_mut() {
                        q.scroll = q.scroll.min(i);
                    }
                    Action::None
                }
                Row::ResetCategory(c) => Action::ResetCategory(c),
                Row::Item(flat) => {
                    if (INFO_FIRST..INFO_FIRST + super::settings::Info::ALL.len()).contains(&flat) {
                        if let Some(k) = rbox {
                            return Action::InfoMove(flat - INFO_FIRST, k == 0);
                        }
                    }
                    if let (true, Some(f)) = (settings::SLIDERS.contains(&flat), slider_f) {
                        return Action::SettingSlider(flat, f);
                    }
                    Action::SettingClick(flat)
                }
            },
            PanelKind::Entities => match panel.selected.clone() {
                Some(kind) => {
                    if i == 0 {
                        panel.selected = None;
                        panel.scroll = 0;
                        return Action::None;
                    }
                    Action::EntityEdit(kind, ENTITY_FIELDS[i - 1])
                }
                None => {
                    let kind = self.panel_rows[i].clone();
                    match rbox {
                        Some(0) => Action::EntityEdit(kind, EntityField::Mini),
                        Some(_) => Action::EntityEdit(kind, EntityField::Map),
                        None => {
                            panel.selected = Some(kind);
                            panel.scroll = 0;
                            panel.search_focus = false;
                            Action::None
                        }
                    }
                }
            },
            PanelKind::Dimensions => {
                let d = self.panel_rows[i].clone();
                self.panel = None;
                Action::ViewDimension(if i == 0 { None } else { Some(d) })
            }
        }
    }

    // ---- layout ----

    pub fn button_rect(&self, i: usize) -> Rect {
        Rect::new(self.screen.x + self.screen.w - 22.0, self.screen.y + 20.0 + i as f32 * 20.0, 18.0, 18.0)
    }
    pub fn list_rect(&self) -> Rect {
        Rect::new(self.screen.x + 4.0, self.screen.y + 18.0, 170.0, (self.screen.h - 40.0).max(60.0))
    }
    pub fn search_rect(&self) -> Rect {
        let l = self.list_rect();
        Rect::new(l.x + 2.0, l.y + 2.0, l.w - 4.0, 13.0)
    }
    pub fn new_button_rect(&self) -> Rect {
        let l = self.list_rect();
        Rect::new(l.x + 2.0, l.y + l.h - 15.0, l.w - 4.0, 13.0)
    }
    /// Rows that fit between the search box and the New button.
    pub fn list_rows(&self) -> usize {
        ((self.list_rect().h - 34.0) / ROW).max(1.0) as usize
    }
    pub fn row_rect(&self, slot: usize) -> Rect {
        let l = self.list_rect();
        Rect::new(l.x + 2.0, l.y + 17.0 + slot as f32 * ROW, l.w - 4.0, ROW)
    }
    /// The small "edit" box at a row's right end.
    pub fn row_edit_rect(&self, slot: usize) -> Rect {
        let r = self.row_rect(slot);
        Rect::new(r.x + r.w - 11.0, r.y + 1.0, 10.0, ROW - 2.0)
    }
    pub fn menu_rect(&self, m: &Menu) -> Rect {
        let (w, h) = (128.0, (m.items().len() + m.header_rows()) as f32 * ROW + 4.0);
        let x = m.at[0].min(self.screen.x + self.screen.w - w - 2.0);
        let y = m.at[1].min(self.screen.y + self.screen.h - h - 2.0);
        Rect::new(x, y, w, h)
    }
    pub fn menu_item_rect(&self, m: &Menu, i: usize) -> Rect {
        let r = self.menu_rect(m);
        Rect::new(r.x + 2.0, r.y + 2.0 + (i + m.header_rows()) as f32 * ROW, r.w - 4.0, ROW)
    }
    /// The visible waypoint of the shown dimension whose mark is nearest to `p`, within `WP_PICK_PX`.
    pub fn waypoint_at(&self, p: [f32; 2], wps: &Waypoints) -> Option<usize> {
        wps.list
            .iter()
            .enumerate()
            .filter(|(_, w)| w.visible && w.dimension == self.dimension)
            .map(|(i, w)| {
                let s = self.screen_of(w.x as f64 + 0.5, w.z as f64 + 0.5);
                (i, (s[0] - p[0]).powi(2) + (s[1] - p[1]).powi(2))
            })
            .filter(|&(_, d)| d <= WP_PICK_PX * WP_PICK_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
    pub fn dialog_rect(&self) -> Rect {
        // 252 wide: four buttons of 57 px, so the longest label ("Сохранить", 54 px) stays inside its button.
        let (w, h) = (252.0, 122.0);
        Rect::new(self.screen.x + (self.screen.w - w) / 2.0, self.screen.y + (self.screen.h - h) / 2.0, w, h)
    }
    /// Dialog rows: 0 name field, 1 colours, 2 icons, 3 the "show on the map" box.
    pub fn dialog_field(&self, i: usize) -> Rect {
        let d = self.dialog_rect();
        Rect::new(d.x + 6.0, d.y + 18.0 + i as f32 * 19.0, d.w - 12.0, 16.0)
    }
    /// Colour `i` of the palette in the colour row, and icon `i` in the icon row: picked with one click.
    pub fn dialog_swatch(&self, i: usize) -> Rect {
        let f = self.dialog_field(1);
        Rect::new(f.x + DIALOG_LABEL_W + i as f32 * 17.0, f.y + 1.0, 14.0, 14.0)
    }
    pub fn dialog_icon(&self, i: usize) -> Rect {
        let f = self.dialog_field(2);
        Rect::new(f.x + DIALOG_LABEL_W + i as f32 * 17.0, f.y + 1.0, 14.0, 14.0)
    }
    pub fn dialog_button(&self, i: usize) -> Rect {
        let d = self.dialog_rect();
        let w = (d.w - 12.0 - 3.0 * 4.0) / 4.0;
        Rect::new(d.x + 6.0 + i as f32 * (w + 4.0), d.y + d.h - 20.0, w, 14.0)
    }

    // ---- coordinates ----

    pub fn world_at(&self, p: [f32; 2]) -> [f64; 2] {
        let (cx, cy) = (self.screen.x + self.screen.w / 2.0, self.screen.y + self.screen.h / 2.0);
        [self.center[0] + (p[0] - cx) as f64 * self.bpp, self.center[1] + (p[1] - cy) as f64 * self.bpp]
    }
    pub fn screen_of(&self, x: f64, z: f64) -> [f32; 2] {
        let (cx, cy) = (self.screen.x + self.screen.w / 2.0, self.screen.y + self.screen.h / 2.0);
        [cx + ((x - self.center[0]) / self.bpp) as f32, cy + ((z - self.center[1]) / self.bpp) as f32]
    }
    pub fn cursor_block(&self) -> [i32; 2] {
        let w = self.world_at(self.cursor);
        [w[0].floor() as i32, w[1].floor() as i32]
    }

    pub fn zoom_at(&mut self, p: [f32; 2], factor: f64) {
        let before = self.world_at(p);
        self.bpp = (self.bpp * factor).clamp(MIN_BPP, MAX_BPP);
        let after = self.world_at(p);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
        self.view_generation += 1;
    }

    /// Visible list entries (waypoint indices) after the search filter, nearest first.
    /// Grouped: ordinary waypoints first, then death points, each group nearest first.
    pub fn filtered(&self, wps: &Waypoints) -> Vec<usize> {
        let mut v = wps.filter(&self.search, Some(&self.dimension), self.player);
        // A stable sort keeps the distance order inside each group.
        v.sort_by_key(|&i| wps.list[i].kind == super::waypoints::Kind::Death);
        v
    }

    fn blocks_map(&self, p: [f32; 2]) -> bool {
        self.edit.is_some()
            || self.menu.is_some_and(|m| self.menu_rect(&m).contains(p))
            || self.list_open && self.list_rect().contains(p)
            || (0..BUTTONS.len()).any(|i| self.button_rect(i).contains(p))
            || self.panel.is_some() && self.panel_rect().contains(p)
    }

    // ---- input ----

    pub fn mouse_moved(&mut self, p: [f32; 2]) {
        self.cursor = p;
    }

    pub fn press(&mut self, p: [f32; 2], button: i32, wps: &Waypoints) -> Action {
        self.cursor = p;
        let d = self.dialog_rect();
        let fields = [self.dialog_field(0), self.dialog_field(1), self.dialog_field(2), self.dialog_field(3)];
        let buttons = [self.dialog_button(0), self.dialog_button(1), self.dialog_button(2), self.dialog_button(3)];
        let (sw0, ic0) = (self.dialog_swatch(0), self.dialog_icon(0));
        let swatches = |row: usize, i: usize| {
            let o = if row == 1 { sw0 } else { ic0 };
            Rect::new(o.x + i as f32 * 17.0, o.y, o.w, o.h)
        };
        if let Some(e) = self.edit.as_mut() {
            if button != 0 {
                return Action::None;
            }
            if let Some(i) = (0..color::PALETTE.len()).find(|&i| swatches(1, i).contains(p)) {
                e.draft.color = i;
            } else if let Some(i) = (0..ICONS.len()).find(|&i| swatches(2, i).contains(p)) {
                e.draft.icon = i;
            } else if fields[3].contains(p) {
                e.draft.visible = !e.draft.visible;
            } else if buttons[0].contains(p) {
                let e = self.edit.take().unwrap();
                let mut w = e.draft;
                if w.name.trim().is_empty() {
                    w.name = wps.next_name(&self.wp_name);
                }
                return Action::SaveWaypoint(e.index, w);
            } else if buttons[1].contains(p) {
                let e = self.edit.take().unwrap();
                return e.index.map_or(Action::None, Action::DeleteWaypoint);
            } else if buttons[2].contains(p) {
                if let Some(i) = e.index {
                    return Action::ShareWaypoint(i);
                }
                return Action::ChatPrefill(e.draft.share_text());
            } else if buttons[3].contains(p) || !d.contains(p) {
                self.edit = None;
            }
            return Action::None;
        }
        if let Some(m) = self.menu.take() {
            for (i, &item) in m.items().iter().enumerate() {
                if button == 0 && self.menu_item_rect(&m, i).contains(p) {
                    let [x, z] = m.world;
                    let y = self.player[1].floor() as i32;
                    let wp = m.wp.and_then(|i| wps.list.get(i).map(|w| (i, w)));
                    return match (item, wp) {
                        (MenuItem::WpHere, _) => {
                            self.new_waypoint(wps, [x, y, z]);
                            Action::None
                        }
                        (MenuItem::CoordsChat, _) => Action::ChatPrefill(format!("{x} ~ {z}")),
                        (MenuItem::SharePos, _) => Action::ChatPrefill(format!("[megamap] here @ {x} {y} {z} ({})", super::waypoints::short_dim(&self.dimension))),
                        (MenuItem::Teleport, _) => Action::ChatPrefill(format!("/tp {x} ~ {z}")),
                        (MenuItem::Center, _) => {
                            self.center = [x as f64 + 0.5, z as f64 + 0.5];
                            self.view_generation += 1;
                            Action::None
                        }
                        (MenuItem::WpEdit, Some((i, w))) => {
                            self.edit = Some(Edit { index: Some(i), draft: w.clone() });
                            Action::None
                        }
                        (MenuItem::WpToggle, Some((i, w))) => Action::SaveWaypoint(Some(i), Waypoint { visible: !w.visible, ..w.clone() }),
                        (MenuItem::WpCenter, Some((_, w))) => {
                            self.center = [w.x as f64 + 0.5, w.z as f64 + 0.5];
                            self.view_generation += 1;
                            Action::None
                        }
                        (MenuItem::WpShare, Some((i, _))) => Action::ShareWaypoint(i),
                        (MenuItem::WpTeleport, Some((_, w))) => Action::ChatPrefill(format!("/tp {} {} {}", w.x, w.y, w.z)),
                        (MenuItem::WpDelete, Some((i, _))) => Action::DeleteWaypoint(i),
                        _ => Action::None,
                    };
                }
            }
            if self.menu_rect(&m).contains(p) {
                return Action::None;
            }
            // A click outside only closes the menu.
            return Action::None;
        }
        if self.panel.is_some() && self.panel_rect().contains(p) {
            return self.panel_press(p, button);
        }
        for (i, (b, _, _)) in BUTTONS.iter().enumerate() {
            if button == 0 && self.button_rect(i).contains(p) {
                let mid = [self.screen.x + self.screen.w / 2.0, self.screen.y + self.screen.h / 2.0];
                match b {
                    Button::ZoomIn => self.zoom_at(mid, 0.5),
                    Button::ZoomOut => self.zoom_at(mid, 2.0),
                    Button::Center => {
                        self.center = [self.player[0], self.player[2]];
                        self.view_generation += 1;
                    }
                    Button::List => {
                        self.list_open = !self.list_open;
                        self.search_focus = self.list_open;
                        if self.list_open {
                            self.panel = None;
                        }
                    }
                    Button::Settings => self.toggle_panel(PanelKind::Settings),
                    Button::Entities => self.toggle_panel(PanelKind::Entities),
                    Button::Dimension => self.toggle_panel(PanelKind::Dimensions),
                    Button::Cave => return Action::ToggleCave,
                    Button::Trail => return Action::ToggleTrail,
                    Button::Clouds => return Action::ToggleClouds,
                    Button::Relief => return Action::CycleRelief,
                    Button::Grid => return Action::CycleGrid,
                }
                return Action::None;
            }
        }
        if self.list_open && self.list_rect().contains(p) {
            self.search_focus = self.search_rect().contains(p);
            if button == 0 && self.new_button_rect().contains(p) {
                let pos = [self.player[0].floor() as i32, self.player[1].floor() as i32, self.player[2].floor() as i32];
                self.new_waypoint(wps, pos);
                return Action::None;
            }
            let rows = self.filtered(wps);
            for slot in 0..self.list_rows() {
                let Some(&i) = rows.get(self.scroll + slot) else { break };
                if !self.row_rect(slot).contains(p) {
                    continue;
                }
                let w = &wps.list[i];
                if button == 1 {
                    // The waypoint's menu (0.3.10; a right click opened the editor).
                    self.menu = Some(Menu { at: p, world: [w.x, w.z], wp: Some(i) });
                } else if self.row_edit_rect(slot).contains(p) || self.last_row_click.is_some_and(|(k, t)| k == i && t.elapsed() < DOUBLE_CLICK) {
                    // The pencil, or a second click on the row.
                    self.last_row_click = None;
                    self.edit = Some(Edit { index: Some(i), draft: w.clone() });
                } else {
                    self.last_row_click = Some((i, std::time::Instant::now()));
                    self.center = [w.x as f64 + 0.5, w.z as f64 + 0.5];
                    self.view_generation += 1;
                }
                return Action::None;
            }
            return Action::None;
        }
        self.search_focus = false;
        match button {
            0 => self.dragging = true,
            1 => {
                let w = self.cursor_block();
                self.menu = Some(Menu { at: p, world: w, wp: self.waypoint_at(p, wps) });
            }
            _ => {}
        }
        Action::None
    }

    pub fn release(&mut self, _button: i32) {
        self.dragging = false;
        self.spin_drag = false;
    }

    pub fn drag(&mut self, delta: [f32; 2]) {
        if self.spin_drag {
            self.spin_yaw = (self.spin_yaw + delta[0] * 1.5).rem_euclid(360.0);
            self.spin_pitch = (self.spin_pitch + delta[1] * 1.0).clamp(-45.0, 50.0);
            return;
        }
        if self.dragging && self.edit.is_none() {
            self.center[0] -= delta[0] as f64 * self.bpp;
            self.center[1] -= delta[1] as f64 * self.bpp;
            self.view_generation += 1;
        }
    }

    pub fn scroll(&mut self, p: [f32; 2], dy: f64, wps: &Waypoints) {
        if self.edit.is_some() {
            return;
        }
        if self.entity_preview_rect().is_some_and(|r| r.contains(p)) {
            self.spin_zoom = (self.spin_zoom * if dy > 0.0 { 1.2 } else { 1.0 / 1.2 }).clamp(SPIN_ZOOM_MIN, SPIN_ZOOM_MAX);
            return;
        }
        if self.panel.is_some() && self.panel_rect().contains(p) {
            let max = self.panel_len().saturating_sub(self.panel_slots());
            if let Some(q) = self.panel.as_mut() {
                q.scroll = if dy > 0.0 { q.scroll.saturating_sub(1) } else { (q.scroll + 1).min(max) };
            }
            return;
        }
        if self.list_open && self.list_rect().contains(p) {
            let n = self.filtered(wps).len();
            let max = n.saturating_sub(self.list_rows());
            self.scroll = if dy > 0.0 { self.scroll.saturating_sub(1) } else { (self.scroll + 1).min(max) };
            return;
        }
        if !self.blocks_map(p) {
            self.zoom_at(p, if dy > 0.0 { 0.8 } else { 1.25 });
        }
    }

    /// A key press; `true` when the screen consumed it (Escape with a panel open closes the panel, not the map).
    pub fn key(&mut self, key: &str) -> (bool, Action) {
        let panel_typing = self.panel.as_ref().is_some_and(|q| q.search_focus);
        let typing = self.edit.is_some() || self.search_focus || panel_typing;
        match key {
            "key.keyboard.escape" => {
                if self.edit.take().is_some() || self.menu.take().is_some() {
                    return (true, Action::None);
                }
                if let Some(q) = self.panel.as_mut() {
                    // Back out of an entity's detail first, then close.
                    if q.selected.take().is_none() {
                        self.panel = None;
                    }
                    return (true, Action::None);
                }
                if self.list_open {
                    self.list_open = false;
                    self.search_focus = false;
                    return (true, Action::None);
                }
                (false, Action::Close)
            }
            "key.keyboard.backspace" if typing => {
                match self.edit.as_mut() {
                    Some(e) => {
                        e.draft.name.pop();
                    }
                    None if panel_typing => {
                        if let Some(q) = self.panel.as_mut() {
                            q.search.pop();
                            q.scroll = 0;
                        }
                    }
                    None => {
                        self.search.pop();
                        self.scroll = 0;
                    }
                }
                (true, Action::None)
            }
            "key.keyboard.enter" if self.edit.is_some() => {
                let e = self.edit.take().unwrap();
                (true, Action::SaveWaypoint(e.index, e.draft))
            }
            // While typing every key is ours, so `M` in a name does not close the map.
            _ if typing => (true, Action::None),
            _ => (false, Action::None),
        }
    }

    pub fn char_typed(&mut self, c: char) -> bool {
        if c.is_control() {
            return false;
        }
        if let Some(e) = self.edit.as_mut() {
            if e.draft.name.chars().count() < 32 {
                e.draft.name.push(c);
            }
            return true;
        }
        if let Some(q) = self.panel.as_mut().filter(|q| q.search_focus) {
            if q.search.chars().count() < 24 {
                q.search.push(c);
            }
            q.scroll = 0;
            return true;
        }
        if self.search_focus {
            if self.search.chars().count() < 24 {
                self.search.push(c);
            }
            self.scroll = 0;
            return true;
        }
        false
    }

    pub fn new_waypoint(&mut self, wps: &Waypoints, pos: [i32; 3]) {
        let mut w = Waypoint::new(&wps.next_name(&self.wp_name), pos, &self.dimension);
        w.color = wps.list.len() % color::PALETTE.len();
        self.edit = Some(Edit { index: None, draft: w });
    }

    /// Whether a key typed into a text field should stay away from the game's own bindings.
    pub fn typing(&self) -> bool {
        self.edit.is_some() || self.search_focus || self.panel.as_ref().is_some_and(|q| q.search_focus)
    }
}

impl Default for MapUi {
    fn default() -> Self {
        Self::new()
    }
}
