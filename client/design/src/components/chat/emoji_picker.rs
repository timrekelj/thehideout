//! The emoji picker: a search field, a strip of section buttons (recent, this server's emojis,
//! the nine Unicode groups) and a virtualized grid of 32pt cells (the Unicode set is about 1,900
//! emojis, so only the rows near the viewport are built), plus [`EmojiSuggest`], the list the
//! composer shows for `:sm`.
//!
//! It is a body, not a popover: the app wraps it in `rune_kit::Popover` (composer, reactions) or
//! drops it into a dialog (the channel icon picker's Emoji and Server emoji tabs, via
//! [`Scope`]). Data in, callbacks out: the caller owns an [`EmojiPickerState`] and the recent
//! list, and hears about a choice through `pick`.
//!
//! Animated server emojis sit still in the grid and play while the pointer is over their cell
//! (one frame at rest keeps a grid of thirty GIFs from waking the window); under reduced motion
//! they never play. Unicode emojis are text and draw in the system's colour emoji font.
//!
//! Keyboard: the search field keeps focus. Arrows move the highlighted cell (Left/Right by one,
//! Up/Down by a row, skipping section titles), Enter picks it, Escape closes.

use std::ops::Range;
use std::rc::Rc;

use emojis::Emoji;
use rune::{col, el, icon, row, text, El, GrowthSpec, Key, ScrollSpec, TextEdit, TextStyle, Tx};
use rune_kit::Kit;

use super::emoji::{
    group_emojis, groups, normalize_query, search_server, search_unicode, unicode_label,
    EmojiImage, EmojiSet, Pick, ServerEmoji, Suggestion, RECENT_MAX,
};
use crate::components::util::over;
use crate::icons::{
    AIRPLANE, CLOCK, FLAG, FORK_KNIFE, HAND_WAVING, HEART, LIGHTBULB, PAW_PRINT, SEARCH, SMILEY,
    SOCCER_BALL, STAR,
};
use crate::theme_of;
use crate::tokens::{icon as size, radius, type_};

/// Cells per grid row.
pub const COLS: usize = 9;
/// A grid cell's side.
pub const CELL: f32 = 32.0;
/// Rows the grid shows unless told otherwise.
pub const ROWS: usize = 8;
/// The body's width: nine cells.
pub const WIDTH: f32 = CELL * COLS as f32;
/// A section button in the strip.
const STRIP_BTN: f32 = 26.0;
/// Side of a custom emoji's picture in a cell.
const PICTURE: f32 = 24.0;
const GLYPH: TextStyle = TextStyle::new(22.0, 400.0).lh(28.0);

/// Which emojis a picker offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Recent, the server's and the Unicode set (composer, reactions).
    All,
    /// Only the Unicode set (a DM, the channel icon's Emoji tab).
    Unicode,
    /// Only the server's emojis (the channel icon's Server emoji tab).
    Server,
}

/// What the caller keeps between frames: the search field and the keyboard-chosen cell.
pub struct EmojiPickerState {
    pub search: TextEdit,
    /// The highlighted cell, counted across the sections as laid out (`None` until a key moves it).
    pub highlight: Option<usize>,
}

impl Default for EmojiPickerState {
    fn default() -> Self {
        EmojiPickerState::new()
    }
}

impl EmojiPickerState {
    /// A fresh state with the search field focused, so typing filters at once.
    pub fn new() -> EmojiPickerState {
        let mut search = TextEdit::new("");
        search.focused = true;
        EmojiPickerState {
            search,
            highlight: None,
        }
    }

    /// The search text changed: the best match is highlighted.
    pub fn changed(&mut self) {
        self.highlight = (!self.search.text.trim().is_empty()).then_some(0);
    }
}

/// A cell: a Unicode emoji or the index of a server emoji in the layout's list.
#[derive(Clone, Copy)]
enum Cell {
    Unicode(&'static Emoji),
    Server(usize),
}

struct Section {
    title: String,
    /// Shown on the strip button that jumps here.
    glyph: &'static rune::Icon,
    cells: Vec<Cell>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    /// The title of section `.0`.
    Title(usize),
    /// Cells `range` (counted across sections) of section `.0`.
    Cells(usize, usize, usize),
}

/// The cells in their sections and the grid rows they fall into: everything the grid and its
/// keyboard need, without any element.
pub struct Layout {
    sections: Vec<Section>,
    server: Vec<ServerEmoji>,
    lines: Vec<Line>,
    total: usize,
}

fn group_glyph(g: emojis::Group) -> &'static rune::Icon {
    use emojis::Group::*;
    match g {
        SmileysAndEmotion => &SMILEY,
        PeopleAndBody => &HAND_WAVING,
        AnimalsAndNature => &PAW_PRINT,
        FoodAndDrink => &FORK_KNIFE,
        TravelAndPlaces => &AIRPLANE,
        Activities => &SOCCER_BALL,
        Objects => &LIGHTBULB,
        Symbols => &HEART,
        Flags => &FLAG,
    }
}

impl Layout {
    /// Lays out `scope` for `query`: with no query, recent (when any), the server's and the
    /// groups, each under a title; with one, the matches of the server's and Unicode.
    pub fn new(scope: Scope, query: &str, server: &[ServerEmoji], recents: &[String]) -> Layout {
        let server_vec = server.to_vec();
        let q = normalize_query(query);
        let mut sections: Vec<Section> = Vec::new();
        let want_server = scope != Scope::Unicode;
        let want_unicode = scope != Scope::Server;
        if q.is_empty() {
            let mut recent = Vec::new();
            for key in recents.iter().take(RECENT_MAX) {
                match Pick::from_key(key, &server_vec) {
                    Some(Pick::Unicode(s)) if want_unicode => {
                        if let Some(e) = emojis::get(s) {
                            recent.push(Cell::Unicode(e));
                        }
                    }
                    Some(Pick::Custom(c)) if want_server => {
                        if let Some(i) = server_vec.iter().position(|e| e.id == c.id) {
                            recent.push(Cell::Server(i));
                        }
                    }
                    _ => {}
                }
            }
            if !recent.is_empty() {
                sections.push(Section {
                    title: "Recent".into(),
                    glyph: &CLOCK,
                    cells: recent,
                });
            }
            if want_server && !server_vec.is_empty() {
                sections.push(Section {
                    title: "This server".into(),
                    glyph: &STAR,
                    cells: (0..server_vec.len()).map(Cell::Server).collect(),
                });
            }
            if want_unicode {
                for (g, title) in groups() {
                    sections.push(Section {
                        title: title.into(),
                        glyph: group_glyph(g),
                        cells: group_emojis(g).map(Cell::Unicode).collect(),
                    });
                }
            }
        } else {
            if want_server {
                let hits: Vec<Cell> = search_server(&q, &server_vec)
                    .into_iter()
                    .filter_map(|e| server_vec.iter().position(|s| s.id == e.id))
                    .map(Cell::Server)
                    .collect();
                if !hits.is_empty() {
                    sections.push(Section {
                        title: "This server".into(),
                        glyph: &STAR,
                        cells: hits,
                    });
                }
            }
            if want_unicode {
                let hits: Vec<Cell> = search_unicode(&q).into_iter().map(Cell::Unicode).collect();
                if !hits.is_empty() {
                    sections.push(Section {
                        title: "Emoji".into(),
                        glyph: &SMILEY,
                        cells: hits,
                    });
                }
            }
        }
        // Titles separate sections unless one section is all there is.
        let titled = sections.len() > 1;
        let mut lines = Vec::new();
        let mut flat = 0;
        for (i, s) in sections.iter().enumerate() {
            if titled {
                lines.push(Line::Title(i));
            }
            let mut at = 0;
            while at < s.cells.len() {
                let len = COLS.min(s.cells.len() - at);
                lines.push(Line::Cells(i, flat + at, len));
                at += len;
            }
            flat += s.cells.len();
        }
        Layout {
            sections,
            server: server_vec,
            lines,
            total: flat,
        }
    }

    /// How many cells there are.
    pub fn len(&self) -> usize {
        self.total
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    fn cell(&self, index: usize) -> Option<Cell> {
        let mut left = index;
        for s in &self.sections {
            if left < s.cells.len() {
                return Some(s.cells[left]);
            }
            left -= s.cells.len();
        }
        None
    }

    /// What cell `index` stands for.
    pub fn pick(&self, index: usize) -> Option<Pick> {
        Some(match self.cell(index)? {
            Cell::Unicode(e) => Pick::Unicode(e.as_str()),
            Cell::Server(i) => Pick::Custom(self.server[i].clone()),
        })
    }

    /// The grid line cell `index` is on.
    pub fn line_of(&self, index: usize) -> Option<usize> {
        self.lines.iter().position(
            |l| matches!(l, Line::Cells(_, start, len) if (*start..*start + *len).contains(&index)),
        )
    }

    /// The grid line a section starts on (its title, or its first row).
    pub fn section_line(&self, section: usize) -> Option<usize> {
        self.lines.iter().position(|l| match l {
            Line::Title(s) | Line::Cells(s, _, _) => *s == section,
        })
    }

    /// Where an arrow key takes the highlight: `dx` cells along, or `dy` rows, keeping the column
    /// where the next row is long enough and clamping to its last cell where it isn't. Starts at
    /// the first cell when nothing is highlighted.
    pub fn step(&self, from: Option<usize>, dx: i32, dy: i32) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let Some(from) = from else { return Some(0) };
        let from = from.min(self.total - 1);
        if dx != 0 {
            return Some((from as i32 + dx).clamp(0, self.total as i32 - 1) as usize);
        }
        let line = self.line_of(from)?;
        let Line::Cells(_, start, _) = self.lines[line] else {
            return Some(from);
        };
        let col = from - start;
        let mut at = line as i32;
        loop {
            at += dy.signum();
            if at < 0 || at >= self.lines.len() as i32 {
                return Some(from);
            }
            if let Line::Cells(_, s, len) = self.lines[at as usize] {
                return Some(s + col.min(len - 1));
            }
        }
    }

    fn range_of(&self, section: usize) -> Range<usize> {
        let start: usize = self.sections[..section].iter().map(|s| s.cells.len()).sum();
        start..start + self.sections[section].cells.len()
    }
}

/// The element key of grid line `line` in the picker keyed `key`, for `Tx::scroll_into_view`.
pub fn row_key(key: &str, line: usize) -> String {
    format!("{key}-row-{line}")
}

type Apply = Rc<dyn Fn(El, usize) -> El>;
/// What a picked cell does: the app's state, the emoji and the transaction.
type PickFn<S> = Rc<dyn Fn(&mut S, Pick, &mut Tx)>;

/// The picker body. Build it with [`EmojiPicker::build`].
pub struct EmojiPicker {
    key: String,
    scope: Scope,
    layout: Rc<Layout>,
    set: EmojiSet,
    search: TextEdit,
    highlight: Option<usize>,
    rows: usize,
    placeholder: String,
}

impl EmojiPicker {
    /// `key` prefixes every element key (one picker per window at a time). `recents` are
    /// reaction keys (`u:…` / `c:…`), newest first.
    pub fn new(
        key: impl Into<String>,
        state: &EmojiPickerState,
        scope: Scope,
        server: &[ServerEmoji],
        recents: &[String],
        set: EmojiSet,
    ) -> EmojiPicker {
        EmojiPicker {
            key: key.into(),
            scope,
            layout: Rc::new(Layout::new(scope, &state.search.text, server, recents)),
            set,
            search: state.search.clone(),
            highlight: state.highlight,
            rows: ROWS,
            placeholder: "Search emoji".into(),
        }
    }

    /// Rows the grid shows (default [`ROWS`]); the rest scroll.
    pub fn visible_rows(mut self, rows: usize) -> Self {
        self.rows = rows.max(1);
        self
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// `state` finds the caller's [`EmojiPickerState`]; `pick` hears a choice (a click or Enter);
    /// `close` hears Escape.
    pub fn build<S: 'static>(
        self,
        k: &Kit,
        state: fn(&mut S) -> &mut EmojiPickerState,
        pick: impl Fn(&mut S, Pick, &mut Tx) + 'static,
        close: impl Fn(&mut S) + 'static,
    ) -> El {
        let key = self.key.clone();
        let layout = self.layout.clone();
        let pick: PickFn<S> = Rc::new(pick);
        let searching = !normalize_query(&self.search.text).is_empty();

        let lay = layout.clone();
        let (pick_enter, pick_key) = (pick.clone(), key.clone());
        let field = rune_kit::TextInput::new(&self.search)
            .placeholder(self.placeholder.clone())
            .label("Search emoji")
            .leading_icon(&SEARCH)
            .large()
            .width(None)
            .on_edit(move |s: &mut S| &mut state(s).search)
            .on_change(move |s: &mut S, _tx| state(s).changed())
            .on_submit({
                let lay = layout.clone();
                move |s: &mut S, tx| {
                    let st = state(s);
                    let at = st.highlight.or((!lay.is_empty()).then_some(0));
                    if let Some(p) = at.and_then(|i| lay.pick(i)) {
                        pick_enter(s, p, tx);
                    }
                }
            })
            .on_key(move |s: &mut S, ev, tx| {
                let (dx, dy) = match ev.key {
                    Key::Left => (-1, 0),
                    Key::Right => (1, 0),
                    Key::Up => (0, -1),
                    Key::Down => (0, 1),
                    Key::Escape => {
                        close(s);
                        return true;
                    }
                    _ => return false,
                };
                let st = state(s);
                st.highlight = lay.step(st.highlight, dx, dy);
                if let Some(line) = st.highlight.and_then(|h| lay.line_of(h)) {
                    tx.scroll_into_view(row_key(&pick_key, line));
                }
                true
            })
            .build(k);

        let mut c = col()
            .key(self.key.clone())
            .w(WIDTH + 16.0)
            .padding(8.0)
            .gap(8.0)
            .child(field);
        if !searching && layout.sections.len() > 1 {
            c = c.child(self.strip::<S>(k));
        }
        c = c.child(self.grid::<S>(k, pick));
        c.child(self.preview(k))
    }

    /// The buttons that jump to a section.
    fn strip<S: 'static>(&self, k: &Kit) -> El {
        let t = theme_of(k);
        let mut bar = row().key(format!("{}-strip", self.key)).w_full().center_y().role("toolbar").label("Emoji sections");
        for (i, s) in self.layout.sections.iter().enumerate() {
            let layout = self.layout.clone();
            let key = self.key.clone();
            bar = bar.child(
                row()
                    .key(format!("{}-section-{i}", self.key))
                    .role("button")
                    .label(s.title.clone())
                    .size(STRIP_BTN, STRIP_BTN)
                    .center()
                    .radius(radius::SM)
                    .focusable(true)
                    .focus_ring(2.0, -2.0, k.c.border_focus)
                    .hover(t.hover.color())
                    .press(t.pressed.color())
                    .child(icon(s.glyph, size::SM).color(t.text_muted.color()).a11y_hidden())
                    .on_press_tx(move |_: &mut S, tx: &mut Tx| {
                        if let Some(line) = layout.section_line(i) {
                            tx.scroll_into_view(row_key(&key, line));
                        }
                    }),
            );
        }
        bar
    }

    fn grid<S: 'static>(&self, k: &Kit, pick: PickFn<S>) -> El {
        let t = theme_of(k);
        let height = CELL * self.rows as f32;
        if self.layout.is_empty() {
            let msg = if self.scope == Scope::Server && normalize_query(&self.search.text).is_empty() {
                "This server has no custom emojis yet"
            } else {
                "No emoji match"
            };
            return col().key(format!("{}-grid", self.key)).w_full().h(height).center().child(text(msg).font(type_::at(k, type_::CONTROL)).color(t.text_faint.color()));
        }
        let layout = self.layout.clone();
        let (key, set, highlight) = (self.key.clone(), self.set.clone(), self.highlight);
        let focus = k.c.border_focus;
        let kit = k.clone();
        let title_font = type_::at(k, type_::LABEL);
        let list = col().key(format!("{}-rows", self.key)).w_full().growth(GrowthSpec::default()).virtual_rows(
            layout.lines.len(),
            CELL,
            {
                let key = key.clone();
                move |_: &S, i| row_key(&key, i).into()
            },
            move |_: &S, r| {
                let line = layout.lines[r];
                let mut line_el = row().key(row_key(&key, r)).w_full().h(CELL);
                match line {
                    Line::Title(sec) => {
                        line_el = line_el.px(4.0).center_y().child(text(layout.sections[sec].title.clone()).font(title_font).color(t.text_muted.color()));
                    }
                    Line::Cells(sec, start, len) => {
                        let first = layout.range_of(sec).start;
                        for n in start..start + len {
                            let cell = layout.sections[sec].cells[n - first];
                            let marked = highlight == Some(n);
                            let mut c = cell_el(&kit, &set, &layout, cell, marked, focus, &key, n);
                            if let Some(p) = layout.pick(n) {
                                let pick = pick.clone();
                                c = c.on_press_tx(move |s: &mut S, tx: &mut Tx| pick(s, p.clone(), tx));
                            }
                            line_el = line_el.child(c);
                        }
                    }
                }
                line_el
            },
        );
        col().key(format!("{}-grid", self.key)).w_full().h(height).scroll(ScrollSpec { stick_bottom: false }).child(list)
    }

    /// The name of the highlighted emoji, or a hint.
    fn preview(&self, k: &Kit) -> El {
        let t = theme_of(k);
        let mut line = row().key(format!("{}-preview", self.key)).w_full().h(24.0).gap(8.0).center_y().px(4.0);
        match self.highlight.and_then(|h| self.layout.pick(h)) {
            Some(p) => {
                line = line.child(text(p.label()).font(type_::at(k, type_::CONTROL)).color(t.text.color()).max_lines(1));
            }
            None => {
                line = line.child(text("Pick an emoji").font(type_::at(k, type_::CONTROL)).color(t.text_faint.color()));
            }
        }
        line
    }
}

/// A custom emoji's picture, `side` square. An animated one rests on its first frame when
/// `play_on_hover` and a still exists, and plays while the pointer is over the nearest hover
/// group (the cell or the message row); under reduced motion it never plays. While the image
/// downloads a faint square holds its place, so nothing shifts when it lands.
pub fn picture(k: &Kit, set: &EmojiSet, img: &EmojiImage, side: f32, play_on_hover: bool) -> El {
    let t = theme_of(k);
    let image = |p: &rune::Pixels| el().size(side, side).pixels_fit(p, rune::SurfaceFit::Fit).a11y_hidden();
    let Some(live) = img.live.clone() else {
        return el().size(side, side).radius(radius::SM).fill(t.surface_raised.color()).a11y_hidden();
    };
    let at_rest = set.pixels(img).unwrap_or_else(|| live.clone());
    if play_on_hover && img.animated && img.still.is_some() {
        let still = img.still.clone().unwrap_or_else(|| live.clone());
        let mut stack = el().size(side, side).child(image(&still).key("still"));
        if !set.is_reduced() {
            stack = stack.child(image(&live).key("live").absolute(0.0, 0.0).group_opacity(0.0, 1.0));
        }
        return stack;
    }
    image(&at_rest)
}

#[allow(clippy::too_many_arguments)]
fn cell_el(k: &Kit, set: &EmojiSet, layout: &Layout, cell: Cell, marked: bool, focus: rune::Color, key: &str, n: usize) -> El {
    let t = theme_of(k);
    let (label, inner) = match cell {
        Cell::Unicode(e) => (unicode_label(e), text(e.as_str()).font(GLYPH).a11y_hidden()),
        Cell::Server(i) => {
            let e = &layout.server[i];
            let img = set.get(&e.id.to_string()).unwrap_or(EmojiImage { name: e.name.clone(), animated: e.animated, live: None, still: None });
            (format!(":{}:", e.name), picture(k, set, &img, PICTURE, true))
        }
    };
    let mut c = el()
        .key(format!("{key}-cell-{n}"))
        .role("button")
        .label(label)
        .hover_group()
        .size(CELL, CELL)
        .center()
        .radius(radius::MD)
        .focusable(true)
        .focus_ring(2.0, -2.0, focus)
        .child(inner);
    c = if marked {
        c.fill(over(t.hover.color(), rune::Color::TRANSPARENT)).outline(2.0, -2.0, focus)
    } else {
        c.hover(t.hover.color()).press(t.pressed.color())
    };
    c
}

/// The autocomplete list under the composer: up to eight rows of picture and `:name:`, one
/// highlighted (Tab and Enter take it, Escape dismisses it; the composer owns those keys).
pub struct EmojiSuggest {
    key: String,
    items: Vec<Suggestion>,
    highlight: usize,
    set: EmojiSet,
    pick: Option<Apply>,
}

/// A row of the suggestion list.
pub const SUGGEST_ROW: f32 = 28.0;

impl EmojiSuggest {
    pub fn new(key: impl Into<String>, items: &[Suggestion], highlight: usize, set: EmojiSet) -> EmojiSuggest {
        EmojiSuggest { key: key.into(), items: items.to_vec(), highlight, set, pick: None }
    }

    /// Called with the row's index when it is pressed.
    pub fn on_pick<S: 'static>(mut self, f: impl Fn(&mut S, usize) + 'static) -> Self {
        let f = Rc::new(f);
        self.pick = Some(Rc::new(move |e, i| {
            let f = f.clone();
            e.on_press(move |s: &mut S| f(s, i))
        }));
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let mut list = col().key(self.key.clone()).role("listbox").label("Emoji suggestions").w(260.0).padding(4.0);
        for (i, s) in self.items.iter().enumerate() {
            let on = i == self.highlight;
            let art = match &s.pick {
                Pick::Unicode(g) => text(*g).font(TextStyle::new(18.0, 400.0).lh(22.0)).a11y_hidden(),
                Pick::Custom(e) => {
                    let img = self.set.get(&e.id.to_string()).unwrap_or(EmojiImage { name: e.name.clone(), animated: e.animated, live: None, still: None });
                    row().size(22.0, 22.0).center().child(picture(k, &self.set, &img, 20.0, false))
                }
            };
            let mut r = row()
                .key(format!("{}-{i}", self.key))
                .role("option")
                // The composer keeps focus while a row is pressed: the list is its helper.
                .preserve_focus_on_press(true)
                .focus_selected(on)
                .label(format!(":{}:", s.name))
                .w_full()
                .h(SUGGEST_ROW)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .radius(radius::SM)
                .child(row().w(24.0).center().child(art))
                .child(text(format!(":{}:", s.name)).font(type_::at(k, type_::CONTROL)).color(if on { t.accent_text } else { t.text }.color()).max_lines(1).a11y_hidden());
            r = if on { r.fill(t.accent_subtle.color()) } else { r.hover(t.hover.color()).press(t.pressed.color()) };
            if let Some(p) = &self.pick {
                r = p(r, i);
            }
            list = list.child(r);
        }
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Vec<ServerEmoji> {
        vec![ServerEmoji { id: 7, name: "smug_cat".into(), animated: false }, ServerEmoji { id: 9, name: "party_parrot".into(), animated: true }]
    }

    fn titles(l: &Layout) -> Vec<String> {
        l.sections.iter().map(|s| s.title.clone()).collect()
    }

    #[test]
    fn the_full_picker_lists_recent_then_server_then_the_nine_groups() {
        let recents = vec!["c:9".to_owned(), "u:\u{1f44d}".to_owned(), "c:404".to_owned()];
        let l = Layout::new(Scope::All, "", &server(), &recents);
        let t = titles(&l);
        assert_eq!(&t[..3], ["Recent", "This server", "Smileys & emotion"]);
        assert_eq!(t.len(), 2 + 9);
        assert_eq!(l.sections[0].cells.len(), 2, "a recent custom emoji that is gone is skipped");
        assert!(matches!(l.pick(0), Some(Pick::Custom(e)) if e.id == 9), "newest first");
        assert!(l.len() > 1500, "the whole Unicode set, in a grid that only builds visible rows");
    }

    #[test]
    fn scope_limits_what_is_offered() {
        let recents = vec!["c:9".to_owned(), "u:\u{1f44d}".to_owned()];
        let u = Layout::new(Scope::Unicode, "", &server(), &recents);
        assert!(!titles(&u).contains(&"This server".to_owned()));
        assert!(u.sections[0].cells.len() == 1 && matches!(u.pick(0), Some(Pick::Unicode(_))), "only Unicode recents");
        let s = Layout::new(Scope::Server, "", &server(), &recents);
        assert_eq!(titles(&s), ["Recent", "This server"].map(String::from));
        assert!(matches!(s.pick(0), Some(Pick::Custom(_))));
        assert_eq!(s.len(), 1 + 2);
        assert!(Layout::new(Scope::Server, "", &[], &[]).is_empty(), "no custom emojis: nothing to pick");
    }

    #[test]
    fn search_covers_server_and_unicode_by_name_and_shortcode() {
        let l = Layout::new(Scope::All, "par", &server(), &[]);
        assert_eq!(titles(&l)[0], "This server");
        assert!(matches!(l.pick(0), Some(Pick::Custom(e)) if e.name == "party_parrot"), "custom matches lead");
        let l = Layout::new(Scope::All, ":smile:", &server(), &[]);
        assert!(matches!(l.pick(0), Some(Pick::Unicode(_))));
        assert!(matches!(l.pick(0), Some(Pick::Unicode(s)) if emojis::get(s).and_then(|e| e.shortcode()) == Some("smile")), "an exact shortcode first");
        assert!(Layout::new(Scope::All, "zzzzzqq", &server(), &[]).is_empty());
        let only_server = Layout::new(Scope::Server, "smug", &server(), &[]);
        assert_eq!(only_server.len(), 1);
    }

    #[test]
    fn arrow_keys_walk_the_grid_across_titles_and_clamp_at_the_ends() {
        let l = Layout::new(Scope::All, "", &server(), &[]);
        // Server has 2 cells in one row; Smileys follow after a title line.
        assert_eq!(l.step(None, 1, 0), Some(0), "the first key lands on the first cell");
        assert_eq!(l.step(Some(0), 1, 0), Some(1));
        assert_eq!(l.step(Some(0), -1, 0), Some(0), "clamped at the start");
        // Down from the server row skips the "Smileys" title and keeps the column (clamped to the row).
        let down = l.step(Some(1), 0, 1).unwrap();
        assert_eq!(l.line_of(down).unwrap(), l.line_of(2).unwrap(), "the first row of the next section");
        assert_eq!(down, 2 + 1, "same column");
        assert_eq!(l.step(Some(2 + 1), 0, -1), Some(1), "and back up");
        assert_eq!(l.step(Some(0), 0, -1), Some(0), "no row above");
        let last = l.len() - 1;
        assert_eq!(l.step(Some(last), 1, 0), Some(last));
        assert_eq!(l.step(Some(last), 0, 1), Some(last));
        assert_eq!(Layout::new(Scope::Server, "", &[], &[]).step(None, 1, 0), None);
        // A section's button scrolls to its title.
        assert_eq!(l.section_line(1), Some(2), "title, one row of the server, then the next title");
        assert_eq!(l.section_line(0), Some(0));
    }
}
