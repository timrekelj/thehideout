//! The user's look: the design crate's `ThemeSpec` plus the reduce-motion override, kept in the
//! app's prefs under `appearance` as JSON.
//!
//! The prefs are read once, in `app::init`, before the first frame; changes are written by a
//! background thread (`Saver`), never from a handler, `poll` or `view`: Rune keeps file I/O out
//! of the frame loop. The thread shares the one `Prefs` with the network backend, which keeps
//! the session in the same file, so neither can overwrite the other's keys.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use serde::{Deserialize, Serialize};
use thehideout_design::ThemeSpec;

/// The prefs key.
pub const KEY: &str = "appearance";
/// The prefs key of the Voice & audio settings.
pub const VOICE_KEY: &str = "voice";
/// The prefs key of the folded sidebar sections: `{"<guild>": [<section>, ...]}`.
pub const COLLAPSED_KEY: &str = "collapsed_sections";
/// The prefs key of the folded embeds: `[[<message>, <embed>], ...]`, oldest first.
pub const EMBEDS_KEY: &str = "collapsed_embeds";
pub use crate::emoji::RECENTS_KEY;

pub use crate::real::SharedPrefs;

/// Whether to follow the system's reduce-motion setting or force it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReduceMotion {
    #[default]
    System,
    On,
    Off,
}

impl ReduceMotion {
    pub const ALL: [ReduceMotion; 3] = [ReduceMotion::System, ReduceMotion::On, ReduceMotion::Off];

    pub fn name(self) -> &'static str {
        match self {
            ReduceMotion::System => "System",
            ReduceMotion::On => "On",
            ReduceMotion::Off => "Off",
        }
    }
}

/// Everything on the Appearance page. Unknown or missing fields fall back to the defaults, so a
/// file from another version still loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    #[serde(flatten)]
    pub spec: ThemeSpec,
    pub reduce_motion: ReduceMotion,
}

/// The saved look, or the default when there is none (or it doesn't parse).
pub fn load(prefs: &SharedPrefs) -> Look {
    prefs.lock().unwrap_or_else(|e| e.into_inner()).get(KEY).unwrap_or_default()
}

/// The saved voice settings, or the defaults when there are none (or they don't parse).
pub fn load_voice(prefs: &SharedPrefs) -> crate::voice::VoicePrefs {
    prefs.lock().unwrap_or_else(|e| e.into_inner()).get(VOICE_KEY).unwrap_or_default()
}

/// The folded sections as `(guild, section)` pairs; unreadable prefs mean none are folded.
pub fn load_collapsed(prefs: &SharedPrefs) -> HashSet<(u64, u64)> {
    let saved: HashMap<String, Vec<u64>> = prefs.lock().unwrap_or_else(|e| e.into_inner()).get(COLLAPSED_KEY).unwrap_or_default();
    saved.into_iter().filter_map(|(g, v)| g.parse::<u64>().ok().map(|g| (g, v))).flat_map(|(g, v)| v.into_iter().map(move |s| (g, s))).collect()
}

/// The recent emojis (reaction keys, newest first); unreadable prefs mean none.
pub fn load_recents(prefs: &SharedPrefs) -> Vec<String> {
    let mut saved: Vec<String> = prefs.lock().unwrap_or_else(|e| e.into_inner()).get(RECENTS_KEY).unwrap_or_default();
    saved.truncate(thehideout_design::components::chat::emoji::RECENT_MAX);
    saved
}

/// The folded embeds as `(message, embed)` pairs, oldest first; unreadable prefs mean none.
pub fn load_embeds(prefs: &SharedPrefs) -> std::collections::VecDeque<(u64, u32)> {
    let saved: Vec<(u64, u32)> = prefs.lock().unwrap_or_else(|e| e.into_inner()).get(EMBEDS_KEY).unwrap_or_default();
    let skip = saved.len().saturating_sub(crate::attach::EMBED_PREFS_CAP);
    saved.into_iter().skip(skip).collect()
}

enum Save {
    Look(Look),
    Embeds(Vec<(u64, u32)>),
    Collapsed(HashMap<String, Vec<u64>>),
    Recents(Vec<String>),
    Voice(crate::voice::VoicePrefs),
}

/// Writes looks and folded sections to the prefs on its own thread; a burst of changes is
/// written as its last one of each kind.
pub struct Saver {
    tx: Option<Sender<Save>>,
    thread: Option<JoinHandle<()>>,
}

impl Saver {
    pub fn spawn(prefs: SharedPrefs) -> Saver {
        let (tx, rx) = mpsc::channel::<Save>();
        let thread = thread::Builder::new()
            .name("thehideout-prefs".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    let (mut look, mut collapsed, mut recents, mut embeds, mut voice) = (None, None, None, None, None);
                    let mut take = |s: Save| match s {
                        Save::Look(l) => look = Some(l),
                        Save::Collapsed(c) => collapsed = Some(c),
                        Save::Recents(r) => recents = Some(r),
                        Save::Embeds(e) => embeds = Some(e),
                        Save::Voice(v) => voice = Some(v),
                    };
                    take(first);
                    while let Ok(newer) = rx.try_recv() {
                        take(newer);
                    }
                    // A pref that can't be saved only costs the next launch's choice.
                    crate::real::with_prefs(&prefs, |p| {
                        if let Some(l) = look {
                            p.set(KEY, &l).ok();
                        }
                        if let Some(c) = collapsed {
                            p.set(COLLAPSED_KEY, &c).ok();
                        }
                        if let Some(r) = recents {
                            p.set(RECENTS_KEY, &r).ok();
                        }
                        if let Some(e) = embeds {
                            p.set(EMBEDS_KEY, &e).ok();
                        }
                        if let Some(v) = voice {
                            p.set(VOICE_KEY, &v).ok();
                        }
                    });
                }
            })
            .ok();
        Saver { tx: Some(tx), thread }
    }

    pub fn save(&self, look: Look) {
        if let Some(tx) = &self.tx {
            tx.send(Save::Look(look)).ok();
        }
    }

    pub fn save_voice(&self, voice: crate::voice::VoicePrefs) {
        if let Some(tx) = &self.tx {
            tx.send(Save::Voice(voice)).ok();
        }
    }

    pub fn save_embeds(&self, embeds: &std::collections::VecDeque<(u64, u32)>) {
        if let Some(tx) = &self.tx {
            tx.send(Save::Embeds(embeds.iter().copied().collect())).ok();
        }
    }

    pub fn save_recents(&self, recents: &[String]) {
        if let Some(tx) = &self.tx {
            tx.send(Save::Recents(recents.to_vec())).ok();
        }
    }

    pub fn save_collapsed(&self, collapsed: &HashSet<(u64, u64)>) {
        let mut by_guild: HashMap<String, Vec<u64>> = HashMap::new();
        for (g, s) in collapsed {
            by_guild.entry(g.to_string()).or_default().push(*s);
        }
        by_guild.values_mut().for_each(|v| v.sort_unstable());
        if let Some(tx) = &self.tx {
            tx.send(Save::Collapsed(by_guild)).ok();
        }
    }
}

impl Drop for Saver {
    /// Finishes what was queued before the app goes.
    fn drop(&mut self) {
        self.tx.take();
        if let Some(t) = self.thread.take() {
            t.join().ok();
        }
    }
}
