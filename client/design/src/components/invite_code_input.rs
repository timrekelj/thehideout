//! InviteCodeInput: an invite code (`XXXX-XXXX`) shown one character per box, eight boxes with a
//! dash between the fourth and the fifth.
//!
//! One transparent single-line text field lies over the boxes and owns the keys, the caret, IME,
//! selection and the accessibility node (one text field, labelled), so typing fills the next box,
//! Backspace clears the previous one and paste fills them all; the boxes only draw what it holds.
//! Clicking anywhere on the row lands on the field and focuses it. Every change runs the text
//! through [`normalize_code`] (uppercase, `I`/`L` -> `1`, `O` -> `0`, anything outside the Crockford
//! alphabet dropped, `-`/space/`_` ignored, an `/invite/` link reduced to its code), so the field
//! never holds anything but a clean, at most eight character code. Chosen over eight separate
//! fields because focus, paste and screen readers then behave as for any other text input, and
//! it needs nothing platform specific (it is the same on wasm).
//!
//! Boxes are 40x52 (scaled with the text size), `radius-md`, on `surface_raised` with the control
//! edge; the box the next character goes into takes the focus ring while the field is focused.

use std::rc::Rc;

use rune::{edit_access, el, row, text, text_input, Color, El, TextEdit, TextStyle, Tx};
use rune_kit::Kit;

use crate::tokens::{radius, type_};
use crate::{metrics_of, theme_of};

/// Characters in a code.
pub const CODE_LEN: usize = 8;
/// Boxes before the dash.
pub const CODE_GROUP: usize = 4;
/// The characters a code may hold (Crockford base32: no I, L, O or U).
pub const CODE_ALPHABET: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

type Wire = Box<dyn FnOnce(El) -> El>;

/// One typed or pasted character as a code character: uppercased, `I`/`L` read as `1`, `O` as `0`,
/// `None` when the alphabet has no such character (including `-`, spaces and `_`).
fn code_char(c: char) -> Option<char> {
    let c = match c.to_ascii_uppercase() {
        'I' | 'L' => '1',
        'O' => '0',
        c => c,
    };
    CODE_ALPHABET.contains(c).then_some(c)
}

/// The code characters in `s`, in order.
fn filter(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().filter_map(code_char)
}

/// `inserted` added after `existing` as a code field takes it: pasted text holding `/invite/` (a
/// link) counts only from after its last `/invite/`; otherwise the characters are filtered as
/// typed. At most [`CODE_LEN`] characters come back.
pub fn accept_code_input(existing: &str, inserted: &str) -> String {
    let inserted = inserted
        .rfind("/invite/")
        .map_or(inserted, |i| &inserted[i + "/invite/".len()..]);
    filter(existing)
        .chain(filter(inserted))
        .take(CODE_LEN)
        .collect()
}

/// A whole field's text as it should be stored (a pasted link anywhere in it wins).
pub fn normalize_code(raw: &str) -> String {
    accept_code_input("", raw)
}

pub struct InviteCodeInput {
    edit: TextEdit,
    label: String,
    invalid: bool,
    disabled: bool,
    full: bool,
    key: Option<String>,
    wires: Vec<Wire>,
}

impl InviteCodeInput {
    pub fn new(edit: &TextEdit) -> InviteCodeInput {
        InviteCodeInput {
            edit: edit.clone(),
            label: "Invite code".into(),
            invalid: false,
            disabled: false,
            full: false,
            key: None,
            wires: Vec::new(),
        }
    }

    /// The accessible name of the field.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// The boxes take the danger edge (a wrong code).
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Fill the width of the container: the eight boxes share the extra width equally (the dash
    /// and the gaps stay fixed, a box never gets narrower than its natural 40 wide scaled with the
    /// text size less a quarter) and the text field covers the same area. Without it the control is
    /// its natural width, about 396.
    pub fn full_width(mut self) -> Self {
        self.full = true;
        self
    }

    /// The key of the whole control (the editable field inside is `<key>/input`).
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Where the text lives in the app state. Every change is filtered back into that text with
    /// [`normalize_code`], so the state only ever holds a clean code.
    pub fn on_edit<S: 'static>(
        mut self,
        f: impl for<'a> Fn(&'a mut S) -> &'a mut TextEdit + 'static,
    ) -> Self {
        let f = Rc::new(f);
        let g = f.clone();
        self.wires.push(Box::new(move |e: El| {
            e.on_edit(edit_access(move |s: &mut S| f(s)))
                .on_input_change(move |s: &mut S, _tx: &mut Tx| {
                    let edit = g(s);
                    let clean = normalize_code(&edit.text);
                    if clean != edit.text {
                        edit.replace_text(clean);
                    }
                })
        }));
        self
    }

    /// Enter.
    pub fn on_submit<S: 'static>(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.wires.push(Box::new(move |e: El| e.on_submit(f)));
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let m = metrics_of(k);
        let (w, h, gap, dash) = (m.em(40.0), m.em(52.0), 8.0, m.em(12.0));
        let font = m.font(TextStyle {
            size: 22.0,
            line_height: 30.0,
            weight: 500.0,
            ..type_::CODE
        });
        let chars: Vec<char> = filter(&self.edit.text).take(CODE_LEN).collect();
        let focused = self.edit.focused && !self.disabled;
        let next = chars.len().min(CODE_LEN - 1);
        let mut cells = row().key("cells").gap(gap).a11y_hidden();
        if self.full {
            cells = cells.w_full();
        }
        for i in 0..CODE_LEN {
            if i == CODE_GROUP {
                cells = cells.child(
                    row()
                        .key("dash")
                        .size(dash, h)
                        .center()
                        .child(el().size(dash, 2.0).radius(1.0).fill(t.text_muted.color())),
                );
            }
            let here = focused && i == next;
            let edge = if self.invalid {
                t.danger.color()
            } else if here {
                k.c.border_focus
            } else {
                t.control_border.color()
            };
            let mut cell = row()
                .key(format!("cell{i}"))
                .h(h)
                .center()
                .radius(radius::MD)
                .fill(t.surface_raised.color())
                .border(if here { 2.0 } else { 1.0 }, edge);
            cell = if self.full { cell.w(w * 0.75).grow(1.0) } else { cell.w(w) };
            if let Some(c) = chars.get(i) {
                cell = cell.child(text(c.to_string()).font(font).color(t.text.color()));
            }
            cells = cells.child(cell);
        }
        let total = w * CODE_LEN as f32 + gap * (CODE_LEN - 1) as f32 + (dash + gap);
        let mut field = text_input(&self.edit, font)
            .single_line()
            .key("input")
            .label(self.label.clone())
            .disabled(self.disabled)
            .color(Color::TRANSPARENT)
            .caret(Color::TRANSPARENT, Color::TRANSPARENT)
            .placeholder("", Color::TRANSPARENT)
            .absolute(0.0, 0.0)
            .h(h);
        field = if self.full { field.w_full() } else { field.w(total) };
        if !self.disabled {
            for wire in self.wires {
                field = wire(field);
            }
        }
        let mut e = el().h(h).child(cells).child(field);
        e = if self.full { e.w_full() } else { e.w(total) };
        if let Some(key) = self.key {
            e = e.key(key);
        }
        if self.disabled {
            e.opacity(0.5)
        } else {
            e
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_uppercases_and_maps_lookalikes() {
        assert_eq!(accept_code_input("", "n0x6"), "N0X6");
        assert_eq!(accept_code_input("", "ilo"), "110");
        assert_eq!(accept_code_input("AB", "cd"), "ABCD");
    }

    #[test]
    fn pasted_codes_lose_separators() {
        assert_eq!(accept_code_input("", "n0x6-pfnb"), "N0X6PFNB");
        assert_eq!(accept_code_input("", "N0X6 PFNB"), "N0X6PFNB");
        assert_eq!(accept_code_input("", "n0x6_pfnb"), "N0X6PFNB");
    }

    #[test]
    fn a_link_gives_the_part_after_the_last_invite() {
        assert_eq!(
            accept_code_input("", "https://hideout.chat/invite/N0X6-PFNB"),
            "N0X6PFNB"
        );
        assert_eq!(
            accept_code_input("", "https://x.y/invite/AAAA/invite/N0X6-PFNB"),
            "N0X6PFNB"
        );
        assert_eq!(accept_code_input("AB", "/invite/CDEF"), "ABCDEF");
    }

    #[test]
    fn other_characters_are_rejected_and_the_length_is_capped() {
        assert_eq!(accept_code_input("", "u!@#é4"), "4");
        assert_eq!(accept_code_input("", "0123456789"), "01234567");
        assert_eq!(accept_code_input("0123456", "89"), "01234568");
        assert_eq!(accept_code_input("", ""), "");
    }

    #[test]
    fn normalizing_is_idempotent() {
        let once = normalize_code("https://h.chat/invite/n0x6-pfnbzz");
        assert_eq!(once, "N0X6PFNB");
        assert_eq!(normalize_code(&once), once);
    }
}
