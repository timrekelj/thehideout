//! Pickers for a channel's look: [`PickerTabs`] (Icons | Emoji | Server emoji), [`IconGrid`] (the
//! Phosphor catalog as a virtualized grid) and [`ColorSwatches`] (the 12 palette keys and "none").
//! Data in, callbacks out; the search field is a plain `TextInput` the caller owns.

use std::rc::Rc;

use rune::{col, el, icon, row, text, El, GrowthSpec, ScrollSpec};
use rune_kit::Kit;

use crate::components::util::over;
use crate::icons::catalog::CatalogIcon;
use crate::icons::{CHECK, X};
use crate::theme_of;
use crate::tokens::palette::{palette_color, PALETTE_KEYS};
use crate::tokens::{icon as size, motion, radius, type_};

/// Icons per grid row.
pub const GRID_COLS: usize = 10;
/// A grid cell's side.
pub const CELL: f32 = 36.0;
/// The grid's visible height: six rows.
pub const GRID_H: f32 = CELL * GRID_ROWS as f32;
/// Rows the grid shows unless told otherwise.
pub const GRID_ROWS: usize = 6;

/// The grid row that holds result `index`.
pub fn row_of(index: usize) -> usize {
    index / GRID_COLS
}

/// The element key of grid row `row`, for `Tx::scroll_into_view`.
pub fn row_key(row: usize) -> String {
    format!("icon-row-{row}")
}

type PickIcon = Rc<dyn Fn(El, &'static str) -> El>;
type PickKey = Rc<dyn Fn(El, &'static str) -> El>;

/// Tabs over the picker: `(label, enabled)`; a disabled one shows "Soon" and cannot be chosen.
pub struct PickerTabs {
    tabs: Vec<(String, bool)>,
    selected: usize,
    pick: Option<Rc<dyn Fn(El, usize) -> El>>,
}

impl PickerTabs {
    pub fn new(tabs: &[(&str, bool)], selected: usize) -> PickerTabs {
        PickerTabs {
            tabs: tabs.iter().map(|(l, e)| ((*l).to_owned(), *e)).collect(),
            selected,
            pick: None,
        }
    }
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
        let mut bar = row()
            .key("picker-tabs")
            .role("tablist")
            .focus_group()
            .w_full()
            .h(32.0)
            .gap(4.0)
            .center_y();
        for (i, (label, enabled)) in self.tabs.into_iter().enumerate() {
            let on = i == self.selected;
            let ink = if !enabled {
                t.text_faint
            } else if on {
                t.text
            } else {
                t.text_muted
            };
            let mut tab = row()
                .key(format!("picker-tab-{i}"))
                .role("tab")
                .focus_selected(on)
                .label(if enabled {
                    label.clone()
                } else {
                    format!("{label}, coming soon")
                })
                .h_full()
                .px(10.0)
                .gap(6.0)
                .center_y()
                .radius(radius::SM)
                .focusable(enabled)
                .focus_ring(2.0, -2.0, k.c.border_focus)
                .token_motion(motion::CONTROL)
                .child(
                    text(label)
                        .font(if on {
                            type_::at(k, type_::CONTROL_STRONG)
                        } else {
                            type_::at(k, type_::CONTROL)
                        })
                        .color(ink.color())
                        .a11y_hidden(),
                );
            if !enabled {
                tab = tab
                    .child(
                        text("Soon")
                            .font(type_::at(k, type_::CAPTION))
                            .color(t.text_faint.color())
                            .a11y_hidden(),
                    )
                    .disabled(true);
            } else {
                tab = tab.hover(t.hover.color()).press(t.pressed.color());
                if let Some(p) = &self.pick {
                    tab = p(tab, i);
                }
            }
            if on {
                tab = tab.border_sides([0.0, 0.0, 2.0, 0.0], t.accent.color());
            }
            bar = bar.child(tab);
        }
        bar
    }
}

/// The Phosphor catalog (or a search's results) as a scrolling grid of `CELL` squares. Only the
/// rows near the viewport are built. `highlight` is the keyboard-chosen result (the search field
/// keeps focus, so the cell is marked rather than focused).
pub struct IconGrid {
    results: Rc<Vec<&'static CatalogIcon>>,
    selected: String,
    highlight: Option<usize>,
    pick: Option<PickIcon>,
    visible_rows: usize,
}

impl IconGrid {
    pub fn new(results: Rc<Vec<&'static CatalogIcon>>, selected: impl Into<String>) -> IconGrid {
        IconGrid {
            results,
            selected: selected.into(),
            highlight: None,
            pick: None,
            visible_rows: GRID_ROWS,
        }
    }
    /// How many rows are visible (default [`GRID_ROWS`]); the rest scroll.
    pub fn visible_rows(mut self, rows: usize) -> Self {
        self.visible_rows = rows.max(1);
        self
    }
    pub fn highlight(mut self, index: Option<usize>) -> Self {
        self.highlight = index;
        self
    }
    pub fn on_pick<S: 'static>(mut self, f: impl Fn(&mut S, &'static str) + 'static) -> Self {
        let f = Rc::new(f);
        self.pick = Some(Rc::new(move |e, name| {
            let f = f.clone();
            e.on_press(move |s: &mut S| f(s, name))
        }));
        self
    }

    pub fn build<S: 'static>(self, k: &Kit) -> El {
        let t = theme_of(k);
        if self.results.is_empty() {
            return col().key("icon-grid").w_full().h(CELL * self.visible_rows as f32).center().child(
                text("No icons match")
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.text_faint.color()),
            );
        }
        let rows = self.results.len().div_ceil(GRID_COLS);
        let height = CELL * self.visible_rows as f32;
        let (results, selected, highlight, pick) = (
            self.results.clone(),
            self.selected,
            self.highlight,
            self.pick,
        );
        let (ink, focus) = (t.text_muted, k.c.border_focus);
        let list = col()
            .key("icon-rows")
            .w_full()
            .growth(GrowthSpec::default())
            .virtual_rows(
                rows,
                CELL,
                |_: &S, i| format!("icon-row-{i}").into(),
                move |_: &S, r| {
                    let mut line = row().key(format!("icon-row-{r}")).w_full().h(CELL);
                    for (n, c) in results
                        .iter()
                        .enumerate()
                        .skip(r * GRID_COLS)
                        .take(GRID_COLS)
                    {
                        let on = c.icon.name == selected;
                        let marked = highlight == Some(n);
                        let name = c.icon.name;
                        let mut cell = el()
                            .key(format!("icon-{name}"))
                            .role("button")
                            .label(name.replace('-', " "))
                            .focus_selected(on)
                            .size(CELL, CELL)
                            .center()
                            .radius(radius::MD)
                            .focusable(true)
                            .focus_ring(2.0, -2.0, focus)
                            .child(
                                icon(&c.icon, size::MD)
                                    .color(if on { t.accent_text } else { ink }.color())
                                    .a11y_hidden(),
                            );
                        cell = if on {
                            cell.fill(t.accent_subtle.color())
                        } else if marked {
                            cell.fill(over(t.hover.color(), rune::Color::TRANSPARENT))
                                .outline(2.0, -2.0, focus)
                        } else {
                            cell.hover(t.hover.color()).press(t.pressed.color())
                        };
                        if let Some(p) = &pick {
                            cell = p(cell, name);
                        }
                        line = line.child(cell);
                    }
                    line
                },
            );
        col()
            .key("icon-grid")
            .w_full()
            .h(height)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(list)
    }
}

/// The 12 palette colours and "none", one swatch each. The selected one carries a check.
pub struct ColorSwatches {
    selected: String,
    pick: Option<PickKey>,
}

impl ColorSwatches {
    /// `selected` is a palette key, or `""` for none.
    pub fn new(selected: impl Into<String>) -> ColorSwatches {
        ColorSwatches {
            selected: selected.into(),
            pick: None,
        }
    }
    /// Called with the palette key (`""` for none).
    pub fn on_pick<S: 'static>(mut self, f: impl Fn(&mut S, &'static str) + 'static) -> Self {
        let f = Rc::new(f);
        self.pick = Some(Rc::new(move |e, key| {
            let f = f.clone();
            e.on_press(move |s: &mut S| f(s, key))
        }));
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let mut bar = row()
            .key("color-swatches")
            .role("radiogroup")
            .label("Colour")
            .focus_group()
            .w_full()
            .flex_wrap()
            .gap(6.0)
            .center_y();
        for key in std::iter::once("").chain(PALETTE_KEYS) {
            let on = self.selected == key;
            let tone = palette_color(key, t);
            let mut swatch = el()
                .key(format!(
                    "swatch-{}",
                    if key.is_empty() { "none" } else { key }
                ))
                .role("radio")
                .focus_selected(on)
                .label(if key.is_empty() { "No colour" } else { key })
                .size(24.0, 24.0)
                .center()
                .radius(radius::MD)
                .focusable(true)
                .focus_ring(crate::space::RING_WIDTH, crate::space::RING_OFFSET, k.c.border_focus)
                .token_motion(motion::CONTROL)
                .hover(t.hover.color());
            swatch = match tone {
                Some(tone) => swatch.fill(tone.color()),
                None => swatch.border(1.0, t.control_border.color()),
            };
            if on {
                // The selection ring sits outside the swatch so the colour stays whole.
                swatch = swatch.outline(crate::space::RING_WIDTH, crate::space::RING_OFFSET, t.text.color());
                swatch = swatch.child(
                    icon(if tone.is_some() { &CHECK } else { &X }, size::XS)
                        .color(if tone.is_some() { t.bg } else { t.text_muted }.color())
                        .a11y_hidden(),
                );
            } else if tone.is_none() {
                swatch = swatch.child(icon(&X, size::XS).color(t.text_faint.color()).a11y_hidden());
            }
            if let Some(p) = &self.pick {
                swatch = p(swatch, key);
            }
            bar = bar.child(swatch);
        }
        bar
    }
}
