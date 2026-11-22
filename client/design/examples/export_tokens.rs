//! Exports the design tokens as JSON for the native mobile apps.
//!
//!     cargo run -p thehideout-design --example export_tokens -- ../../mobile/tokens/tokens.json
//!
//! Colour is not reproducible from a few deltas (`fit` walks lightness until contrast passes), so
//! every look (8 accents x 5 backgrounds x dark/light x tint x contrast = 320) is exported fully
//! resolved to sRGB hex, exactly as desktop computes it. Everything else (radii, spacing, type,
//! metrics, motion, palette, avatars, shadows) is exported as plain numbers.

use std::fmt::Write;

use serde_json::{json, Map, Value};
use thehideout_design::tokens::{
    accent::AccentPreset,
    background::BackgroundPreset,
    elevation::{shadows, Level},
    metrics::{Density, Metrics, TextSize},
    motion, palette, radius, space,
    spec::{Contrast, ThemeSpec},
    theme::Theme,
    type_,
};

fn kebab(s: &str) -> String {
    s.to_ascii_lowercase().replace([' ', '_'], "-")
}

fn accent_key(a: AccentPreset) -> String {
    kebab(a.name())
}

fn bg_key(b: BackgroundPreset) -> String {
    kebab(b.name())
}

fn style(s: rune::TextStyle) -> Value {
    json!({
        "size": s.size,
        "weight": s.weight,
        "lineHeight": s.line_height,
        "tracking": s.tracking,
        "mono": matches!(s.family, rune::Family::Mono),
    })
}

/// serde_json widens f32 to f64 (0.2 becomes 0.20000000298); print the shortest f32 instead.
fn clean(v: Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => {
            let f = n.as_f64().unwrap() as f32;
            format!("{f}").parse::<f64>().ok().and_then(serde_json::Number::from_f64).map_or(Value::Number(n), Value::Number)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(clean).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, clean(v))).collect()),
        v => v,
    }
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: export_tokens <out.json>");

    // Token order is the order of `Theme::tokens()` followed by the extras below.
    let probe = Theme::build(&ThemeSpec::default(), true);
    let mut order: Vec<String> = probe.tokens().iter().map(|(n, _)| n.to_string()).collect();
    for extra in ["layer1", "layer2", "layer3", "knob", "knob_on"] {
        order.push(extra.to_owned());
    }

    let mut looks = Map::new();
    let mut palettes = Map::new();
    for (spec, dark) in ThemeSpec::all_looks() {
        let t = Theme::build(&spec, dark);
        let key = format!(
            "{}/{}/{}/{}/{}",
            accent_key(spec.accent),
            bg_key(spec.background),
            if dark { "dark" } else { "light" },
            if spec.tint { "tint" } else { "plain" },
            if spec.contrast == Contrast::High {
                "high"
            } else {
                "standard"
            },
        );
        let mut toks = Map::new();
        for (n, tone) in t.tokens() {
            toks.insert(n.to_owned(), json!(tone.hex()));
        }
        for (i, l) in t.layers.iter().enumerate() {
            toks.insert(format!("layer{}", i + 1), json!(l.hex()));
        }
        toks.insert("knob".into(), json!(t.knob.hex()));
        toks.insert("knob_on".into(), json!(t.knob_on.hex()));
        looks.insert(key.clone(), Value::Object(toks));

        // Channel palette: icon ink and text-safe ink per key; the wash is `ink` at WASH_ALPHA.
        let mut pal = Map::new();
        for k in palette::PALETTE_KEYS {
            pal.insert(
                k.to_owned(),
                json!({
                    "ink": palette::palette_color(k, &t).unwrap().hex(),
                    "text": palette::palette_text(k, &t).unwrap().hex(),
                }),
            );
        }
        palettes.insert(key, Value::Object(pal));
    }

    let mut avatars = Map::new();
    let mut accents = Vec::new();
    for a in AccentPreset::ALL {
        avatars.insert(
            accent_key(a),
            json!(a
                .palette()
                .avatars
                .iter()
                .map(|t| t.hex())
                .collect::<Vec<_>>()),
        );
        accents.push(json!({ "key": accent_key(a), "name": a.name(), "wire": serde_json::to_value(a).unwrap() }));
    }
    let backgrounds: Vec<Value> = BackgroundPreset::ALL
        .iter()
        .map(|b| json!({ "key": bg_key(*b), "name": b.name() }))
        .collect();

    // Metrics for every density x text size.
    let mut metrics = Map::new();
    for m in Metrics::all() {
        let d = match m.density {
            Density::Comfortable => "comfortable",
            Density::Compact => "compact",
        };
        let ts = match m.text {
            TextSize::P90 => "90",
            TextSize::P100 => "100",
            TextSize::P110 => "110",
            TextSize::P125 => "125",
        };
        let mut scaled = Map::new();
        for (name, s) in type_::SCALE {
            scaled.insert(name.to_owned(), style(m.font(s)));
        }
        let (h_top, h_bot) = m.msg_pad(true);
        let (f_top, f_bot) = m.msg_pad(false);
        metrics.insert(
            format!("{d}/{ts}"),
            json!({
                "rowHeight": m.row_h(),
                "rowGap": m.row_gap(),
                "controlHeight": m.control_h(),
                "sectionHeight": m.section_h(),
                "fieldHeight": m.field_h(),
                "zonePad": m.zone_pad(),
                "headerHeight": m.header_h(),
                "footerHeight": m.footer_h(),
                "groupGap": m.group_gap(),
                "msgPadHead": [h_top, h_bot],
                "msgPadFollow": [f_top, f_bot],
                "msgAvatar": m.msg_avatar(),
                "railTile": m.rail_tile(),
                "tableRow56": m.table_row(56.0),
                "pad16": m.pad(16.0),
                "pad12": m.pad(12.0),
                "pad8": m.pad(8.0),
                "em": { "10": m.em(10.0), "20": m.em(20.0), "40": m.em(40.0), "48": m.em(48.0) },
                "icon": { "14": m.icon(14.0), "16": m.icon(16.0), "20": m.icon(20.0) },
                "type": scaled,
            }),
        );
    }

    let spring = |s: rune::Spring| json!({ "duration": s.duration, "bounce": s.bounce });
    let motion_tokens: Map<String, Value> = motion::ALL
        .iter()
        .map(|(n, s)| (n.trim_start_matches("motion-").to_owned(), spring(*s)))
        .collect();

    let shadow = |level: Level, dark: bool| {
        shadows(level, dark)
            .iter()
            .map(|s| json!({ "x": s.x, "y": s.y, "blur": s.blur, "spread": s.spread, "color": s.color.hex() }))
            .collect::<Vec<_>>()
    };

    let doc = json!({
        "version": 1,
        "generatedBy": "client/design/examples/export_tokens.rs; do not edit",
        "defaults": {
            "accent": "sage-teal", "background": "warm", "appearance": "system",
            "tint": false, "contrast": "standard", "density": "comfortable", "textSize": "100",
        },
        "axes": {
            "accents": accents,
            "backgrounds": backgrounds,
            "appearances": ["system", "light", "dark"],
            "contrasts": ["standard", "high"],
            "densities": ["comfortable", "compact"],
            "textSizes": ["90", "100", "110", "125"],
            "textSizeFactors": { "90": 0.9, "100": 1.0, "110": 1.1, "125": 1.25 },
        },
        "tokenOrder": order,
        "looks": looks,
        "channelPalette": {
            "keys": palette::PALETTE_KEYS,
            "washAlpha": palette::WASH_ALPHA,
            "looks": palettes,
        },
        "avatars": avatars,
        "radii": radius::SCALE.iter().map(|(n, v)| (n.trim_start_matches("radius-").to_owned(), json!(v))).collect::<Map<_, _>>(),
        "radiusFull": radius::FULL,
        "spacing": space::SCALE.iter().map(|(n, v)| (n.trim_start_matches("space-").to_owned(), json!(v))).collect::<Map<_, _>>(),
        "ring": { "width": space::RING_WIDTH, "offset": space::RING_OFFSET },
        "icon": { "xs": 14, "sm": 16, "md": 20 },
        "type": {
            "families": { "ui": "Instrument Sans", "mono": "JetBrains Mono" },
            "fontFiles": {
                "ui": ["InstrumentSans-VF.ttf"],
                "mono": ["JetBrainsMono-Regular.ttf", "JetBrainsMono-Bold.ttf"],
            },
            "scale": type_::SCALE.iter().map(|(n, s)| (n.to_string(), style(*s))).collect::<Map<_, _>>(),
            "extra": {
                "control-strong": style(type_::CONTROL_STRONG),
            },
        },
        "metrics": metrics,
        "motion": motion_tokens,
        "elevation": {
            "popover": { "dark": shadow(Level::Popover, true), "light": shadow(Level::Popover, false) },
            "modal": { "dark": shadow(Level::Modal, true), "light": shadow(Level::Modal, false) },
        },
    });

    let doc = clean(doc);
    let mut s = String::new();
    write!(s, "{}", serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    s.push('\n');
    std::fs::write(&out, s).expect("write");
    eprintln!(
        "wrote {out}: {} looks",
        doc["looks"].as_object().unwrap().len()
    );
}
