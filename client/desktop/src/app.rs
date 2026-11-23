//! The app root: state plus backend, the poll that carries replies into state, key handling and
//! the top-level view (auth or main, dialogs and toasts on top).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rune::{Cx, El, Key, KeyEvent, SpringPair, Transition, Tx, Waker, el};
use thehideout_api::{Event, GatewayUpdate};
use thehideout_design::{kit_for, motion, theme_for};

use crate::apply::apply;
use crate::backend::{Backend, Command, Reply};
use crate::fake::{FakeBackend, now_unix};
use crate::format::Tz;
use crate::look::{self, Saver};
use crate::real::{RealBackend, SharedPrefs, open_prefs};
use crate::state::{AppState, Screen};
use crate::ui;
use crate::voice::{self, VoiceSink};

/// Rune's `S`: everything handlers can reach.
pub struct App {
    pub state: AppState,
    pub backend: Box<dyn Backend>,
    /// Launch sequence of the network backend: 0 done (or fake), 1 before the first poll, 2 after
    /// it. The saved session is checked on the poll after the first frame has been drawn.
    boot: u8,
    /// Writes look changes to the prefs on a background thread. `None` in scenes and tests.
    saver: Option<Saver>,
    /// The audio engine, once one is wired: it gets the state's voice intents on every flush.
    voice_sink: Option<Box<dyn VoiceSink>>,
    /// The server refused this app version: the window shows only the "Update required" screen.
    pub update_required: bool,
}

impl App {
    pub fn new(backend: Box<dyn Backend>, state: AppState) -> App {
        App {
            state,
            backend,
            boot: 0,
            saver: None,
            voice_sink: None,
            update_required: false,
        }
    }

    /// The app as the window starts it: the saved look is read from `prefs` now, before the first
    /// frame, so the window never flashes the default theme; later changes are saved on a thread.
    pub fn start(backend: Box<dyn Backend>, mut state: AppState, prefs: SharedPrefs) -> App {
        state.ui.look = look::load(&prefs);
        state.ui.collapsed = look::load_collapsed(&prefs);
        state.ui.recents = look::load_recents(&prefs);
        state.ui.attach.embeds_collapsed = look::load_embeds(&prefs);
        state.load_voice_prefs(look::load_voice(&prefs));
        let mut app = App::new(backend, state);
        app.saver = Some(Saver::spawn(prefs));
        app
    }

    /// Installs the audio engine that receives the voice intents.
    pub fn set_voice_sink(&mut self, sink: Box<dyn VoiceSink>) {
        self.voice_sink = Some(sink);
    }

    /// The live picture of a video track the stage watches (the sink draws it), if there is one.
    pub fn video_pixels(&self, track: &str) -> Option<rune::Pixels> {
        self.voice_sink.as_ref()?.video_pixels(track)
    }

    /// Hands the commands queued on the state to the backend, and the voice intents to the
    /// audio engine (dropped while there is none).
    pub fn flush(&mut self) {
        self.state.settle_voice_page();
        self.state.settle_video();
        voice::send_intents(&mut self.state, self.voice_sink.as_deref_mut());
        if std::mem::take(&mut self.state.ui.look_dirty)
            && let Some(saver) = &self.saver
        {
            saver.save(self.state.ui.look);
        }
        if std::mem::take(&mut self.state.voice.prefs_dirty)
            && let Some(saver) = &self.saver
        {
            saver.save_voice(self.state.voice_prefs());
        }
        if std::mem::take(&mut self.state.ui.collapsed_dirty)
            && let Some(saver) = &self.saver
        {
            saver.save_collapsed(&self.state.ui.collapsed);
        }
        if std::mem::take(&mut self.state.ui.recents_dirty)
            && let Some(saver) = &self.saver
        {
            saver.save_recents(&self.state.ui.recents);
        }
        if std::mem::take(&mut self.state.ui.attach.embeds_dirty)
            && let Some(saver) = &self.saver
        {
            saver.save_embeds(&self.state.ui.attach.embeds_collapsed);
        }
        for cmd in std::mem::take(&mut self.state.outbox) {
            self.backend.submit(cmd);
        }
    }

    /// A handler's way to change state: runs `f`, then sends what it queued.
    pub fn act(&mut self, f: impl FnOnce(&mut AppState)) {
        f(&mut self.state);
        self.flush();
    }

    /// Applies every reply that is due now, until it goes quiet (scenes and tests; the window
    /// does this from `poll`).
    pub fn pump(&mut self) {
        let mut tx = Tx::default();
        loop {
            self.flush();
            let replies = self.backend.poll(self.state.clock);
            let heard = voice::take_events(&mut self.state, self.voice_sink.as_deref_mut());
            if replies.is_empty() && !heard {
                break;
            }
            for r in replies {
                take(&mut self.state, r, &mut tx);
            }
        }
    }
}

/// Applies one reply. A member joining a server may be an invite just accepted, so the invite
/// lists on screen ask again (seen here, beside `apply`, which keeps the data itself).
fn take(s: &mut AppState, r: Reply, tx: &mut Tx) {
    if let Reply::Gateway(GatewayUpdate::Event(
        Event::GuildMemberJoined { guild_id, .. } | Event::MemberUpdated { guild_id, .. },
    )) = &r
    {
        s.membership_changed(*guild_id);
    }
    apply(s, r, tx);
}

/// The pinned clock of the fake world, scenes and tests.
pub fn fake_now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(now_unix() as u64)
}

/// The real wall clock, for the real backend.
pub fn real_now() -> SystemTime {
    #[cfg(not(target_arch = "wasm32"))]
    {
        SystemTime::now()
    }
    // `SystemTime::now` panics on wasm32-unknown-unknown: ask the page.
    #[cfg(target_arch = "wasm32")]
    {
        UNIX_EPOCH + Duration::from_millis(js_sys::Date::now() as u64)
    }
}

/// Flags of the runs that draw without a window (scenes, snapshots, scripts).
const HEADLESS: &[&str] = &[
    "--scene",
    "--snap",
    "--script",
    "--tree",
    "--html",
    "--video",
    "--frames",
    "--trace",
    "--strip",
    "--list-scenes",
    "--playground",
    "--first-pixels",
    "--cpu-boot",
];

pub fn headless() -> bool {
    std::env::args()
        .skip(1)
        .any(|a| HEADLESS.contains(&a.as_str()))
}

/// Whether this run uses the deterministic fake: `--fake`, or any headless run (scenes,
/// snapshots, scripts) unless `--real` asks for the network.
fn wants_fake() -> bool {
    let extra = rune::extra_args();
    if extra.iter().any(|a| a == "--fake") {
        return true;
    }
    if extra.iter().any(|a| a == "--real") {
        return false;
    }
    headless()
}

/// The window's first state: the network backend with a loading screen while the saved session
/// is checked, or signed out on the fake backend. The look is read from the prefs here, once,
/// before any frame; a headless run never touches the user's prefs, so snapshots stay the same
/// on every machine.
pub fn init() -> App {
    if wants_fake() {
        let state = AppState::new(fake_now, Tz::Local);
        let backend = Box::new(FakeBackend::new().with_latency(0.25));
        if headless() {
            return App::new(backend, state);
        }
        let mut app = App::start(backend, state, open_prefs());
        // The fake world's audio: a device list and a fixed mic level, no real audio.
        app.set_voice_sink(Box::new(crate::fake::FakeAudio::demo()));
        return app;
    }
    let prefs = open_prefs();
    let mut state = AppState::new(real_now, Tz::Local);
    state.screen = Screen::Loading;
    let mut app = App::start(
        Box::new(RealBackend::with_shared(prefs.clone())),
        state,
        prefs,
    );
    app.boot = 1;
    // Only the network backend gets the audio engine (built on the first join); the fake world
    // and every scene stay silent.
    #[cfg(not(target_arch = "wasm32"))]
    app.set_voice_sink(Box::new(crate::voice_engine::app_sink()));
    #[cfg(target_arch = "wasm32")]
    app.set_voice_sink(Box::new(crate::voice_web::web_sink()));
    app
}

/// The window: the fonts, the handlers and the first state. Desktop `main` adds its titlebar
/// and open-URL handler; the browser entry runs it as is.
pub fn spec() -> rune::App<App> {
    use rune::AppSpec;
    use thehideout_design::{FAMILIES, FONTS};
    let app = rune::App::new(AppSpec::<App> {
        fonts: FONTS,
        families: FAMILIES,
        min_size: Some((800.0, 600.0)),
        #[cfg(not(target_arch = "wasm32"))]
        scenes: crate::scenes::SCENES,
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("thehideout", (1280.0, 800.0), init, view)
    });
    // Async browser work (a finished file chooser) reaches the app through this handle.
    #[cfg(target_arch = "wasm32")]
    crate::web::set_handle(app.handle());
    app
}

pub fn setup(a: &mut App, waker: Waker) {
    // Pictures decode off the frame loop and wake the window when they are ready.
    a.state.data.media.set_waker(waker.clone());
    a.state.ui.attach.waker = Some(waker.clone());
    if let Some(sink) = &mut a.voice_sink {
        sink.set_waker(waker.clone());
    }
    a.backend.set_waker(waker);
}

/// Before every frame: replies in, time-driven state forward. True when something visible changed.
pub fn poll(a: &mut App, clock: f64, tx: &mut Tx) -> bool {
    match a.boot {
        // The first poll comes before the first frame: wait for the next.
        1 => {
            a.boot = 2;
            tx.wake_after(Duration::from_millis(1));
        }
        2 => {
            a.boot = 0;
            a.state.outbox.push(Command::Resume);
        }
        _ => {}
    }
    let mut changed = false;
    loop {
        a.flush();
        let replies = a.backend.poll(clock);
        let heard = voice::take_events(&mut a.state, a.voice_sink.as_deref_mut());
        if replies.is_empty() && !heard {
            break;
        }
        changed = true;
        for r in replies {
            take(&mut a.state, r, tx);
        }
    }
    if !a.update_required && thehideout_api::update_required() {
        a.update_required = true;
        changed = true;
    }
    if let Some(text) = a.state.ui.copy_request.take() {
        tx.copy(text);
    }
    // A playing video's time moves the scrub bar: the view runs about ten times a second meanwhile.
    let playing = a.state.any_clip_playing();
    if playing {
        tx.wake_after(Duration::from_millis(100));
    }
    let (ticked, wake) = a.state.tick(clock);
    if let Some(due) = [wake, a.backend.next_due()]
        .into_iter()
        .flatten()
        .reduce(f64::min)
    {
        tx.wake_at(due);
    }
    changed || ticked || playing
}

/// A link the system opened the app with (`thehideout://invite/<token>`, once the scheme is
/// registered with the system): signed out, it opens that invite.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn on_open_url(a: &mut App, url: &str, _tx: &mut Tx) {
    a.act(|s| s.open_link(url));
}

/// Keys no focused control took: Cmd/Ctrl+W closes the window, Cmd/Ctrl+Q quits, Alt+Up and
/// Alt+Down move between channels, Escape leaves the settings, Cmd/Ctrl+Shift+A opens the admin dashboard (admins only), and the voice hotkeys
/// (mute, deafen and push to talk, rebindable on the Voice & audio page) work. (Escape belongs to an open modal
/// or menu first.)
pub fn on_key(a: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Command('w') => tx.close_window(),
        Key::Command('q') => tx.quit(),
        Key::Command(c) if c.eq_ignore_ascii_case(&'a') && ev.mods.shift && a.state.is_admin() => {
            a.act(|s| s.open_admin())
        }
        Key::Escape if a.state.ui.attach.lightbox.is_some() => a.act(|s| s.close_lightbox()),
        Key::Left if a.state.ui.attach.lightbox.is_some() => a.act(|s| s.lightbox_step(-1)),
        Key::Right if a.state.ui.attach.lightbox.is_some() => a.act(|s| s.lightbox_step(1)),
        Key::Escape
            if a.state
                .ui
                .admin
                .as_ref()
                .is_some_and(|ad| ad.confirm.is_some()) =>
        {
            a.act(|s| s.cancel_admin_confirm())
        }
        Key::Escape if a.state.ui.admin.is_some() => a.act(|s| s.close_admin()),
        Key::Escape if a.state.ui.settings.is_some() => a.act(|s| s.close_settings()),
        Key::Escape if a.state.ui.server_settings.is_some() => a.act(|s| s.close_server_settings()),
        _ if a.state.voice_hotkey(ev, tx.physical_key(), tx.key_repeat()) => a.flush(),
        Key::Up if ev.mods.alt && a.state.screen == Screen::Main => a.act(|s| s.step_channel(-1)),
        Key::Down if ev.mods.alt && a.state.screen == Screen::Main => a.act(|s| s.step_channel(1)),
        _ => return false,
    }
    true
}

#[cfg(test)]
thread_local! {
    static VIEWS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many times the view has run on this thread (tests: a scroll must not add to it).
#[cfg(test)]
pub fn views() -> u64 {
    VIEWS.with(|v| v.get())
}

pub fn view(a: &App, cx: &mut Cx<App>) -> El {
    #[cfg(test)]
    VIEWS.with(|v| v.set(v.get() + 1));
    let look = a.state.ui.look;
    let dark = look.spec.is_dark(cx.system_dark());
    let (k, t) = (kit_for(&look.spec, dark), theme_for(&look.spec, dark));
    if a.update_required {
        return rune_kit::root(ui::update_required::build(a, k).fill(t.bg.color()), []);
    }
    // Reduce motion follows the system unless the user forced it (the override lasts one commit,
    // so it is restated every time the view runs).
    match look.reduce_motion {
        look::ReduceMotion::System => {}
        look::ReduceMotion::On => cx.reduced_motion(true),
        look::ReduceMotion::Off => cx.reduced_motion(false),
    }
    // Pictures read it while the tree is built: animated emojis rest on their first frame.
    a.state.ui.reduced.set(match look.reduce_motion {
        look::ReduceMotion::On => true,
        look::ReduceMotion::Off => false,
        look::ReduceMotion::System => cx.is_reduced_motion(),
    });
    let lay = cx
        .layout_with("stage", |r| ui::shell::Layout::of(r[2], r[3]))
        .unwrap_or_default();
    let mut layer = Vec::new();
    // Auth and the shell share the window; each fades on `motion-overlay` and nothing else moves.
    let fade = Transition::opacity().springs(SpringPair::new(motion::OVERLAY, motion::OVERLAY));
    let screen = match a.state.screen {
        Screen::Loading => el().key("screen-loading"),
        Screen::Auth => el().key("screen-auth").child(ui::auth::build(a, k)),
        // New terms to accept: nothing of the app is reachable until they are.
        Screen::Main if a.state.terms_prompt_due() => el().key("screen-terms").child(ui::account::terms_screen(a, k)),
        Screen::Main => el().key("screen-main").child(ui::shell::build(
            a,
            k,
            &mut layer,
            cx.chrome_insets().height,
            lay,
        )),
    };
    let screen = screen.absolute(0.0, 0.0).w_full().h_full().transition(fade);
    ui::dialogs::build(a, k, &mut layer, lay.height);
    ui::voice_stage::picker(a, k, &mut layer);
    ui::security::dialogs(a, k, &mut layer, lay.height);
    ui::account::dialogs(a, k, &mut layer, lay.height);
    ui::emoji::build(a, k, &mut layer);
    ui::attach::lightbox_layer(a, k, &mut layer, (lay.width, lay.height));
    layer.push(ui::toasts::build(a, k));
    // Where the window's own controls sit over the content: a strip that drags the window.
    let mut stage = el()
        .key("stage")
        .w_full()
        .h_full()
        .fill(t.bg.color())
        .child(screen);
    let bar = cx.chrome_insets().height;
    if bar > 0.0 {
        stage = stage.child(
            el().key("titlebar")
                .absolute(0.0, 0.0)
                .w_full()
                .h(bar)
                .window_drag(),
        );
    }
    // The push-to-talk key's release, and the window going quiet, which would otherwise leave the
    // mic open: both are heard on the root (a release no focused control took).
    rune_kit::root(stage, layer)
        .on_key_up(|a: &mut App, ev: &KeyEvent, tx: &mut Tx| {
            let taken = a.state.voice_key_up(ev, tx.physical_key());
            if taken {
                a.flush();
            }
            taken
        })
        .on_window_focus(|a: &mut App, focused: bool, _tx: &mut Tx| {
            a.act(|s| s.voice_window_focus(focused))
        })
}
