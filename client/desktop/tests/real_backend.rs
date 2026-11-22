//! `RealBackend` against the real server running in-process on a throwaway ScyllaDB keyspace
//! (needs ScyllaDB at 127.0.0.1:9042: `docker compose up -d`). The backend is driven the way the
//! app drives it: commands in, replies out of `poll`.
//!
//! The binary has no library target, so the modules the backend (and, for the reconnect test, the
//! app state that reacts to it) needs are included by path.

#![allow(dead_code)]

#[path = "../src/account.rs"]
mod account;
#[path = "../src/admin.rs"]
mod admin;
#[path = "../src/apply.rs"]
mod apply;
#[path = "../src/attach.rs"]
mod attach;
#[path = "../src/backend.rs"]
mod backend;
#[path = "../src/emoji.rs"]
mod emoji;
#[path = "../src/format.rs"]
mod format;
#[path = "../src/forms.rs"]
mod forms;
#[path = "../src/layout.rs"]
mod layout;
#[path = "../src/look.rs"]
mod look;
#[path = "../src/media.rs"]
mod media;
#[path = "../src/moderation.rs"]
mod moderation;
#[path = "../src/perms.rs"]
mod perms;
#[path = "../src/real.rs"]
mod real;
#[path = "../src/server_settings.rs"]
mod server_settings;
#[path = "../src/state.rs"]
mod state;
#[path = "../src/structure.rs"]
mod structure;
#[path = "../src/voice.rs"]
mod voice;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use backend::{Backend, Command, Reply, Session};
use rand::Rng;
use real::RealBackend;
use rune::prefs::Prefs;
use scylla::client::caching_session::CachingSession;
use scylla::client::session_builder::SessionBuilder;
use thehideout_api::{ApiError, Event, GatewayUpdate};
use thehideout_server::config::S3Config;
use thehideout_server::snowflake::Snowflake;
use thehideout_server::storage::Storage;
use thehideout_server::{AppState, build_app, migrations};
use tokio::net::TcpListener;

const PASSWORD: &str = "correct horse battery";
const WAIT: Duration = Duration::from_secs(10);

struct Server {
    url: String,
    state: AppState,
    /// The runtime the server runs on, to create accounts from the (sync) test thread.
    runtime: tokio::runtime::Handle,
}

impl Server {
    async fn start() -> Self {
        Self::boot(false, false).await
    }

    /// With voice on, signing tokens for the dev LiveKit keys of `.env.example` (no media flows).
    async fn start_with_voice() -> Self {
        Self::boot(false, true).await
    }

    /// With the S3-compatible store attached, for pictures (`docker compose up -d`).
    async fn start_with_media() -> Self {
        Self::boot(true, false).await
    }

    async fn boot(media: bool, voice: bool) -> Self {
        let storage = if media {
            let config = S3Config::from_env().expect("S3 config");
            let storage = Storage::s3(&config).expect("configure S3 storage");
            storage.check().await.unwrap_or_else(|e| {
                panic!(
                    "object storage at {} is not reachable ({e:#}); run `docker compose up -d`",
                    config.endpoint
                )
            });
            Some(storage)
        } else {
            None
        };
        let session = SessionBuilder::new()
            .known_node("127.0.0.1:9042")
            .build()
            .await
            .expect("connect to ScyllaDB at 127.0.0.1:9042 (is `docker compose up -d` running?)");
        let keyspace = format!("test_{:016x}", rand::rng().random::<u64>());
        migrations::run(&session, &keyspace)
            .await
            .expect("run migrations");
        let session = Arc::new(CachingSession::from(session, 64));
        let mut state = AppState::new(
            session,
            Arc::new(Snowflake::new(0)),
            keyspace,
            Duration::from_millis(200),
        );
        state.storage = storage;
        if voice {
            let config = thehideout_server::config::LiveKitConfig::new(
                "ws://127.0.0.1:7880",
                None,
                "devkey".into(),
                "secret".into(),
            )
            .expect("livekit config");
            state = state.with_livekit(thehideout_server::voice::LiveKit::new(config));
        }
        let app = build_app(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Self {
            url: format!("http://{addr}"),
            state,
            runtime: tokio::runtime::Handle::current(),
        }
    }

    /// Creates the account `username` on the server: sign-up is invite-only, so
    /// tests create accounts directly and then sign in.
    async fn create_account(&self, username: &str) {
        let email = format!("{username}@example.test");
        thehideout_server::accounts::create_user(
            &self.state,
            thehideout_server::accounts::NewUser {
                username,
                password: Some(PASSWORD),
                email: Some(&email),
            },
        )
        .await
        .expect("create account");
    }

    async fn cleanup(self) {
        if let Some(storage) = &self.state.storage
            && let Ok(result) = self
                .state
                .session
                .execute_unpaged("SELECT key FROM media_objects", ())
                .await
            && let Ok(rows) = result.into_rows_result()
            && let Ok(rows) = rows.rows::<(String,)>()
        {
            for (key,) in rows.flatten() {
                let _ = storage.delete(&key).await;
            }
        }
        let _ = self
            .state
            .session
            .get_session()
            .query_unpaged(
                format!("DROP KEYSPACE IF EXISTS {}", self.state.keyspace),
                (),
            )
            .await;
    }
}

/// A user's backend plus every reply it has produced that no one has asked for yet.
struct Peer {
    backend: RealBackend,
    seen: Vec<Reply>,
    wakes: Arc<AtomicUsize>,
    prefs: std::path::PathBuf,
}

impl Peer {
    fn new(url: &str) -> Peer {
        let prefs = std::env::temp_dir().join(format!(
            "thehideout-test-{:016x}.json",
            rand::rng().random::<u64>()
        ));
        let mut peer = Peer::with_prefs(prefs);
        peer.backend.set_server(url);
        peer
    }

    fn with_prefs(prefs: std::path::PathBuf) -> Peer {
        let mut backend = RealBackend::with_prefs(Prefs::at(&prefs));
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&wakes);
        backend.set_waker(Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        }));
        Peer {
            backend,
            seen: Vec::new(),
            wakes,
            prefs,
        }
    }

    /// The first reply (already seen or still to come) that `pick` takes.
    fn wait<T>(&mut self, what: &str, mut pick: impl FnMut(&Reply) -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(i) = self.seen.iter().position(|r| pick(r).is_some()) {
                return pick(&self.seen.remove(i)).unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; unclaimed: {:?}",
                self.seen
            );
            self.seen.extend(self.backend.poll(0.0));
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn ask<T>(&mut self, cmd: Command, pick: impl FnMut(&Reply) -> Option<T>) -> T {
        let what = format!("the answer to {cmd:?}");
        self.backend.submit(cmd);
        self.wait(&what, pick)
    }

    /// Creates the account on the server, then signs in through the backend.
    fn sign_up(&mut self, server: &Server, name: &str) -> Session {
        server.runtime.block_on(server.create_account(name));
        let cmd = Command::Login {
            username: name.into(),
            password: PASSWORD.into(),
        };
        let session = self.ask(cmd, |r| match r {
            Reply::Auth(a) => Some(a.clone()),
            _ => None,
        });
        session.expect("sign in")
    }

    fn initial(&mut self) -> backend::Initial {
        self.ask(Command::LoadInitial, |r| match r {
            Reply::Initial(i) => Some(i.clone()),
            _ => None,
        })
        .expect("initial load")
    }

    fn connected(&mut self) {
        self.wait("the gateway to connect", |r| {
            matches!(r, Reply::Gateway(GatewayUpdate::Connected)).then_some(())
        });
    }

    fn saved(&self) -> Option<std::collections::HashMap<String, String>> {
        Prefs::at(&self.prefs).get("session")
    }
}

#[test]
fn drives_the_real_server() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(Server::start());

    let mut alice = Peer::new(&server.url);
    let mut bob = Peer::new(&server.url);

    // Sign up, and the session lands in the prefs file.
    let a = alice.sign_up(&server, "alice");
    assert_eq!(a.me.username, "alice");
    let saved = alice.saved().expect("the session is saved after sign-up");
    assert_eq!(
        (saved["server_url"].as_str(), saved["token"].as_str()),
        (server.url.as_str(), a.token.as_str())
    );
    assert!(
        alice.wakes.load(Ordering::SeqCst) > 0,
        "the waker rings when a reply lands"
    );
    let init = alice.initial();
    assert!((init.me.clone(), init.guilds.len(), init.dms.len()) == (a.me.clone(), 0, 0));
    alice.connected();

    // A guild with its first channel, and one more.
    let guild = alice
        .ask(
            Command::CreateGuild {
                name: "friends".into(),
            },
            |r| match r {
                Reply::GuildCreated(g) => Some(g.clone()),
                _ => None,
            },
        )
        .expect("create guild");
    let channels = alice
        .ask(Command::LoadChannels { guild: guild.id }, |r| match r {
            Reply::Channels { guild: g, result } if *g == guild.id => Some(result.clone()),
            _ => None,
        })
        .expect("channels")
        .0;
    let general = channels[0].id;
    let extra = alice
        .ask(
            Command::CreateChannel {
                kind: thehideout_api::ChannelKind::Text,
                guild: guild.id,
                name: "random".into(),
                section: None,
                icon: Default::default(),
                color: Default::default(),
            },
            |r| match r {
                Reply::ChannelCreated(c) => Some(c.clone()),
                _ => None,
            },
        )
        .expect("create channel");
    assert_eq!((extra.guild_id, extra.name.as_str()), (guild.id, "random"));

    // Bob signs up, sees nothing, joins by invite code and loads the members.
    let b = bob.sign_up(&server, "bob");
    assert!(bob.initial().guilds.is_empty());
    bob.connected();
    let joined = bob
        .ask(
            Command::JoinGuild {
                code: guild.invite_code.clone(),
            },
            |r| match r {
                Reply::GuildJoined(g) => Some(g.clone()),
                _ => None,
            },
        )
        .expect("join guild");
    assert_eq!(joined.id, guild.id);
    let members = bob
        .ask(Command::LoadMembers { guild: guild.id }, |r| match r {
            Reply::Members { guild: g, result } if *g == guild.id => Some(result.clone()),
            _ => None,
        })
        .expect("members");
    let mut names: Vec<_> = members.iter().map(|u| u.username.as_str()).collect();
    names.sort();
    assert_eq!(names, ["alice", "bob"]);
    alice.wait("bob's join on alice's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::GuildMemberJoined { guild_id, user }))
            if *guild_id == guild.id && user.id == b.me.id =>
        {
            Some(())
        }
        _ => None,
    });

    // A message reaches the other user's gateway with its nonce, and bob's RPC reply carries it too.
    bob.backend.submit(Command::Send {
        channel: general,
        content: "hello alice".into(),
        nonce: "n-1".into(),
    });
    let live = alice.wait("bob's message on alice's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::MessageCreated(m))) if m.nonce == "n-1" => {
            Some(m.clone())
        }
        _ => None,
    });
    assert_eq!(
        (
            live.content.as_str(),
            live.author_id,
            live.channel_id,
            live.guild_id
        ),
        ("hello alice", b.me.id, general, Some(guild.id))
    );
    let sent = bob.wait("bob's send reply", |r| match r {
        Reply::Sent { nonce, result, .. } if nonce == "n-1" => Some(result.clone()),
        _ => None,
    });
    assert_eq!(sent.expect("sent").id, live.id);

    // Pagination: more than a page, walked back with `before`.
    for i in 0..60 {
        let nonce = format!("p-{i}");
        let n = nonce.clone();
        bob.ask(
            Command::Send {
                channel: general,
                content: format!("m{i}"),
                nonce,
            },
            move |r| match r {
                Reply::Sent { nonce, result, .. } if *nonce == n => {
                    Some(result.clone().map(|_| ()))
                }
                _ => None,
            },
        )
        .expect("send");
    }
    let page = |peer: &mut Peer, before| {
        peer.ask(
            Command::LoadMessages {
                channel: general,
                before,
            },
            |r| match r {
                Reply::Messages {
                    channel,
                    before: b,
                    result,
                } if *channel == general && *b == before => Some(result.clone()),
                _ => None,
            },
        )
        .expect("messages")
    };
    let first = page(&mut alice, None);
    assert_eq!((first.items.len(), first.has_more), (50, true));
    assert_eq!(first.items[0].content, "m59", "newest first");
    let second = page(&mut alice, first.next_before);
    assert_eq!((second.items.len(), second.has_more), (11, false));
    assert_eq!(second.items.last().unwrap().content, "hello alice");

    // Direct messages: bob hears of the channel on his gateway and the participants carry names.
    let dm = alice
        .ask(Command::OpenDm { user_id: b.me.id }, |r| match r {
            Reply::DmOpened(d) => Some(d.clone()),
            _ => None,
        })
        .expect("open dm");
    let mut who: Vec<_> = dm
        .participants
        .iter()
        .map(|u| u.username.as_str())
        .collect();
    who.sort();
    assert_eq!(who, ["alice", "bob"]);
    bob.wait("the new DM on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::DmOpened(d))) if d.id == dm.id => Some(()),
        _ => None,
    });

    // Errors come back as replies, not panics.
    let denied = bob.ask(
        Command::CreateChannel {
            kind: thehideout_api::ChannelKind::Text,
            guild: guild.id,
            name: "nope".into(),
            section: None,
            icon: Default::default(),
            color: Default::default(),
        },
        |r| match r {
            Reply::ChannelCreated(c) => Some(c.clone()),
            _ => None,
        },
    );
    assert_eq!(denied.unwrap_err(), ApiError::PermissionDenied);

    // Launch again with the saved session: it is validated and the app is signed in as alice.
    let mut again = Peer::with_prefs(alice.prefs.clone());
    let resumed = again.ask(Command::Resume, |r| match r {
        Reply::Resume(x) => Some(x.clone()),
        _ => None,
    });
    let (url, session) = resumed.expect("resume").expect("a saved session");
    assert_eq!(
        (url.as_str(), session.me.id),
        (server.url.as_str(), a.me.id)
    );
    let init = again.initial();
    assert_eq!(
        (init.guilds.len(), init.channels.len(), init.dms.len()),
        (1, 2, 1)
    );
    again.connected();

    // Sign-out forgets it, and nothing of the old session is delivered afterwards.
    alice.backend.submit(Command::SignOut);
    alice.wait("sign-out", |r| matches!(r, Reply::SignedOut).then_some(()));
    assert!(alice.saved().is_none(), "sign-out clears the saved session");
    let mut fresh = Peer::with_prefs(alice.prefs.clone());
    assert_eq!(
        fresh.ask(Command::Resume, |r| match r {
            Reply::Resume(x) => Some(x.clone()),
            _ => None,
        }),
        Ok(None)
    );

    // A refused token is forgotten, quietly: the form shows without an error.
    let stale = std::env::temp_dir().join(format!(
        "thehideout-test-stale-{:016x}.json",
        rand::rng().random::<u64>()
    ));
    let mut prefs = Prefs::at(&stale);
    prefs
        .set(
            "session",
            &std::collections::HashMap::from([
                ("server_url", server.url.as_str()),
                ("token", "not-a-token"),
            ]),
        )
        .unwrap();
    let mut old = Peer::with_prefs(stale);
    let refused = old.ask(Command::Resume, |r| match r {
        Reply::Resume(x) => Some(x.clone()),
        _ => None,
    });
    assert_eq!(refused, Err(ApiError::Unauthenticated));
    assert!(old.saved().is_none());

    // The server field decides where the next sign-in goes; a dead one is a network error.
    let mut lost = Peer::new("http://127.0.0.1:1");
    let down = lost.ask(
        Command::Login {
            username: "alice".into(),
            password: PASSWORD.into(),
        },
        |r| match r {
            Reply::Auth(a) => Some(a.clone()),
            _ => None,
        },
    );
    assert!(matches!(down, Err(ApiError::Network(_))), "got {down:?}");
    assert!(lost.saved().is_none());
    lost.backend.set_server(&server.url);
    let wrong = lost.ask(
        Command::Login {
            username: "alice".into(),
            password: "wrong password!".into(),
        },
        |r| match r {
            Reply::Auth(a) => Some(a.clone()),
            _ => None,
        },
    );
    assert_eq!(
        wrong,
        Err(ApiError::Unauthenticated),
        "the new server answered"
    );
    let ok = lost.ask(
        Command::Login {
            username: "bob".into(),
            password: PASSWORD.into(),
        },
        |r| match r {
            Reply::Auth(a) => Some(a.clone()),
            _ => None,
        },
    );
    assert_eq!(ok.expect("login").me.id, b.me.id);

    rt.block_on(server.cleanup());
    for p in [&alice, &bob, &again, &fresh, &old, &lost] {
        std::fs::remove_file(&p.prefs).ok();
    }
}

// ---- reconnect --------------------------------------------------------------------------------

/// A TCP forwarder that can be killed (cutting every open connection, like a server restart
/// would) and started again on the same port, as `client/api/tests/live.rs` does.
struct Proxy {
    addr: std::net::SocketAddr,
    target: std::net::SocketAddr,
    tasks: tokio::task::JoinSet<()>,
}

impl Proxy {
    async fn start(target: std::net::SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        Self::serve(listener, addr, target)
    }

    fn serve(
        listener: TcpListener,
        addr: std::net::SocketAddr,
        target: std::net::SocketAddr,
    ) -> Self {
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            let mut conns = tokio::task::JoinSet::new();
            while let Ok((mut inbound, _)) = listener.accept().await {
                conns.spawn(async move {
                    if let Ok(mut outbound) = tokio::net::TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                });
            }
        });
        Self {
            addr,
            target,
            tasks,
        }
    }

    async fn kill(&mut self) {
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
    }

    async fn restart(&mut self) {
        let listener = TcpListener::bind(self.addr)
            .await
            .expect("rebind proxy port");
        *self = Self::serve(listener, self.addr, self.target);
    }
}

/// The app's state wired to the real backend the way `App::pump` wires it to any backend, and the
/// names of the gateway updates it has seen.
struct Desk {
    ui: state::AppState,
    backend: RealBackend,
    gateway: Vec<&'static str>,
    prefs: std::path::PathBuf,
}

impl Desk {
    fn new(url: &str) -> Desk {
        let prefs = std::env::temp_dir().join(format!(
            "thehideout-test-{:016x}.json",
            rand::rng().random::<u64>()
        ));
        let mut ui = state::AppState::new(std::time::SystemTime::now, format::Tz::Local);
        ui.auth.server.replace_text(url);
        Desk {
            ui,
            backend: RealBackend::with_prefs(Prefs::at(&prefs)),
            gateway: Vec::new(),
            prefs,
        }
    }

    fn pump(&mut self) {
        for cmd in std::mem::take(&mut self.ui.outbox) {
            if cmd.signed_out() {
                self.backend.set_server(&self.ui.server_url());
            }
            self.backend.submit(cmd);
        }
        for reply in self.backend.poll(0.0) {
            if let Reply::Gateway(u) = &reply {
                self.gateway.push(match u {
                    GatewayUpdate::Connecting => "Connecting",
                    GatewayUpdate::Connected => "Connected",
                    GatewayUpdate::Disconnected { .. } => "Disconnected",
                    GatewayUpdate::AuthExpired => "AuthExpired",
                    GatewayUpdate::Event(_) => continue,
                });
            }
            apply::apply(&mut self.ui, reply, &mut rune::Tx::default());
        }
    }

    fn until(&mut self, what: &str, cond: impl Fn(&Desk) -> bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            self.pump();
            if cond(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; gateway so far {:?}",
                self.gateway
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn log_has(&self, channel: u64, content: &str) -> usize {
        self.ui.log(channel).map_or(0, |l| {
            l.messages.iter().filter(|m| m.content == content).count()
        })
    }
}

/// The gateway drops and retries (`Connecting` before every attempt, as the real one does), a
/// message is posted meanwhile, and once it is back the app asks for what it missed and shows it
/// once.
#[test]
fn a_dropped_gateway_refetches_what_was_missed() {
    use state::{AuthMode, DialogKind, Place, Screen};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(Server::start());
    let mut proxy = rt.block_on(Proxy::start(
        server.url.trim_start_matches("http://").parse().unwrap(),
    ));
    let url = format!("http://{}", proxy.addr);

    // Alice (invited earlier: the account exists) runs the app through the proxy and makes a server.
    rt.block_on(server.create_account("alice"));
    let mut alice = Desk::new(&url);
    alice.ui.set_auth_mode(AuthMode::Login);
    alice.ui.auth.username.replace_text("alice");
    alice.ui.auth.password.replace_text(PASSWORD);
    alice.ui.submit_auth();
    alice.until("the main screen", |d| {
        d.ui.screen == Screen::Main
            && d.ui.ui.connection == thehideout_design::components::ConnectionStatus::Connected
    });
    alice.ui.open_dialog(DialogKind::CreateGuild);
    alice
        .ui
        .ui
        .dialog
        .as_mut()
        .unwrap()
        .input
        .replace_text("friends");
    alice.ui.submit_dialog();
    alice.until("her server and its first channel", |d| {
        d.ui.current_channel()
            .is_some_and(|c| d.ui.log(c).is_some_and(|l| l.loaded))
    });
    let Place::Guild(guild) = alice.ui.ui.place else {
        panic!("not in the new server")
    };
    let channel = alice.ui.current_channel().unwrap();
    let invite = alice.ui.guild(guild).unwrap().invite_code.clone();

    // The connection is cut and stays down long enough for a retry to fail.
    rt.block_on(proxy.kill());
    alice.until("a failed retry", |d| {
        d.gateway
            .windows(3)
            .any(|w| w == ["Disconnected", "Connecting", "Disconnected"])
    });

    // Bob, straight on the server, posts while alice is away.
    let bob = thehideout_api::Client::new(&server.url);
    rt.block_on(async {
        server.create_account("bob").await;
        bob.login("bob", PASSWORD).await.unwrap();
        bob.join_guild(&invite).await.unwrap();
        bob.send_message(channel, "posted while you were away", None)
            .await
            .unwrap();
    });
    for _ in 0..10 {
        alice.pump();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        alice.log_has(channel, "posted while you were away"),
        0,
        "nothing reaches her while she is away"
    );

    // The connection returns: the sequence ends Connecting, Connected and the refetch goes out.
    let sent_before = alice.backend.sent().len();
    rt.block_on(proxy.restart());
    alice.until("the message", |d| {
        d.log_has(channel, "posted while you were away") > 0
    });
    assert!(
        alice.gateway.ends_with(&["Connecting", "Connected"]),
        "{:?}",
        alice.gateway
    );
    let sent = &alice.backend.sent()[sent_before..];
    assert!(
        sent.contains(&Command::LoadMessages {
            channel,
            before: None
        }),
        "the open channel is fetched again: {sent:?}"
    );
    assert!(
        sent.contains(&Command::Refresh),
        "the lists are fetched again: {sent:?}"
    );
    alice.until("bob in the member list", |d| {
        d.ui.data.members.get(&guild).is_some_and(|m| m.len() == 2)
    });
    let log = alice.ui.log(channel).unwrap();
    assert_eq!(
        log.messages
            .iter()
            .filter(|m| m.content == "posted while you were away")
            .count(),
        1
    );
    let mut ids: Vec<u64> = log.messages.iter().map(|m| m.id).collect();
    ids.dedup();
    assert_eq!(ids.len(), log.messages.len(), "nothing is duplicated");
    assert_eq!(
        alice.ui.ui.place,
        Place::Guild(guild),
        "the selection stays"
    );

    rt.block_on(proxy.kill());
    rt.block_on(server.cleanup());
    std::fs::remove_file(&alice.prefs).ok();
}

/// Sections, a channel's icon and colour, and the layout, through the real server and a second
/// client's gateway: what one user edits, the other sees as events.
#[test]
fn sections_icons_and_layout_reach_a_second_client() {
    use thehideout_api::{ChannelIconRef, PaletteColor};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(Server::start());
    let mut alice = Peer::new(&server.url);
    let mut bob = Peer::new(&server.url);
    alice.sign_up(&server, "alice");
    alice.initial();
    alice.connected();
    let guild = alice
        .ask(
            Command::CreateGuild {
                name: "friends".into(),
            },
            |r| match r {
                Reply::GuildCreated(g) => Some(g.clone()),
                _ => None,
            },
        )
        .expect("create guild");
    bob.sign_up(&server, "bob");
    bob.initial();
    bob.connected();
    bob.ask(
        Command::JoinGuild {
            code: guild.invite_code.clone(),
        },
        |r| matches!(r, Reply::GuildJoined(Ok(_))).then_some(()),
    );
    // Both clients listen for the layout; drop what the setup produced.
    alice.seen.clear();
    bob.seen.clear();
    let guild = guild.id;
    let is_layout = |r: &Reply| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::LayoutUpdated { guild_id, layout }))
            if *guild_id == guild =>
        {
            Some(layout.clone())
        }
        _ => None,
    };

    // Alice creates a section: her reply carries the layout, bob's gateway announces it.
    let (section, layout) = alice
        .ask(
            Command::CreateSection {
                guild,
                name: "Projects".into(),
            },
            |r| match r {
                Reply::SectionSaved { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("create section");
    assert_eq!(section.name, "Projects");
    let seen = bob.wait("the new section on bob's gateway", |r| {
        is_layout(r).filter(|l| l.sections.iter().any(|s| s.id == section.id))
    });
    assert_eq!(seen.version, layout.version);

    // A channel with its own icon and colour, created straight into the section.
    let dev = alice
        .ask(
            Command::CreateChannel {
                kind: thehideout_api::ChannelKind::Text,
                guild,
                name: "dev".into(),
                section: Some(section.id),
                icon: ChannelIconRef::Phosphor("code".into()),
                color: PaletteColor::Blue,
            },
            |r| match r {
                Reply::ChannelCreated(c) => Some(c.clone()),
                _ => None,
            },
        )
        .expect("create channel");
    assert_eq!(
        (dev.section_id, dev.icon.as_str(), dev.color),
        (Some(section.id), "code", PaletteColor::Blue)
    );
    let announced = bob.wait("the channel on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::ChannelCreated(c))) if c.id == dev.id => {
            Some(c.clone())
        }
        _ => None,
    });
    assert_eq!(
        (announced.icon.as_str(), announced.color),
        ("code", PaletteColor::Blue)
    );
    let after_create = bob.wait("the layout after the channel", |r| {
        is_layout(r).filter(|l| layout::channels_in(l, Some(section.id)).contains(&dev.id))
    });

    // Alice moves the channel to the top level with the version she holds; bob sees the new order.
    let mine = layout::normalized(&after_create);
    let moved = layout::move_channel_to(&mine, dev.id, None, 0).expect("a real move");
    let saved = alice
        .ask(
            Command::UpdateLayout {
                guild,
                layout: moved.clone(),
            },
            |r| match r {
                Reply::LayoutSaved { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("update layout");
    assert_eq!(layout::channels_in(&saved, None).first(), Some(&dev.id));
    assert!(
        saved.version > mine.version,
        "the server bumped the version"
    );
    let seen = bob.wait("the move on bob's gateway", |r| {
        is_layout(r).filter(|l| l.version == saved.version)
    });
    assert_eq!(layout::channels_in(&seen, None).first(), Some(&dev.id));

    // The same stale version again is refused: refetch and retry.
    let stale = alice.ask(
        Command::UpdateLayout {
            guild,
            layout: moved,
        },
        |r| match r {
            Reply::LayoutSaved { result, .. } => Some(result.clone()),
            _ => None,
        },
    );
    assert_eq!(stale, Err(ApiError::Conflict));

    // Icon, colour and topic change; bob's gateway carries the new channel.
    let updated = alice
        .ask(
            Command::UpdateChannel {
                guild,
                channel: dev.id,
                name: None,
                icon: Some(ChannelIconRef::Phosphor("rocket".into())),
                color: Some(PaletteColor::Pink),
                topic: Some("ship it".into()),
            },
            |r| match r {
                Reply::ChannelSaved { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("update channel");
    assert_eq!(
        (updated.icon.as_str(), updated.color, updated.topic.as_str()),
        ("rocket", PaletteColor::Pink, "ship it")
    );
    let seen = bob.wait("the update on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::ChannelUpdated(c))) if c.id == dev.id => {
            Some(c.clone())
        }
        _ => None,
    });
    assert_eq!(
        (
            seen.icon.as_str(),
            seen.color,
            seen.topic.as_str(),
            seen.name.as_str()
        ),
        ("rocket", PaletteColor::Pink, "ship it", "dev")
    );

    // Bob may not: he has no role with MANAGE_CHANNELS.
    let denied = bob.ask(
        Command::UpdateChannel {
            guild,
            channel: dev.id,
            name: Some("nope".into()),
            icon: None,
            color: None,
            topic: None,
        },
        |r| match r {
            Reply::ChannelSaved { result, .. } => Some(result.clone()),
            _ => None,
        },
    );
    assert_eq!(denied, Err(ApiError::PermissionDenied));
    let access = bob
        .ask(Command::LoadAccess { guild }, |r| match r {
            Reply::Access { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("access");
    assert!(
        access.member_roles.contains_key(&2) || !access.member_roles.is_empty(),
        "members are listed with their role ids"
    );

    // Deleting the section leaves its channels at the top level.
    let after = alice
        .ask(
            Command::DeleteSection {
                guild,
                section: section.id,
            },
            |r| match r {
                Reply::SectionDeleted { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("delete section");
    assert!(after.sections.is_empty());
    assert!(layout::channels_in(&after, None).contains(&dev.id));

    rt.block_on(server.cleanup());
    std::fs::remove_file(&alice.prefs).ok();
    std::fs::remove_file(&bob.prefs).ok();
}

/// A role with a colour and MANAGE_CHANNELS, handed to a second user: who could not touch the
/// layout before now reorders it. Role order goes through the real server too, stale version and all.
#[test]
fn a_role_with_manage_channels_lets_a_second_user_reorder_the_layout() {
    use thehideout_api::{PaletteColor, Permissions};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(Server::start());
    let mut alice = Peer::new(&server.url);
    let mut bob = Peer::new(&server.url);
    alice.sign_up(&server, "alice");
    alice.initial();
    alice.connected();
    let guild = alice
        .ask(
            Command::CreateGuild {
                name: "friends".into(),
            },
            |r| match r {
                Reply::GuildCreated(g) => Some(g.clone()),
                _ => None,
            },
        )
        .expect("create guild");
    let b = bob.sign_up(&server, "bob");
    bob.initial();
    bob.connected();
    bob.ask(
        Command::JoinGuild {
            code: guild.invite_code.clone(),
        },
        |r| matches!(r, Reply::GuildJoined(Ok(_))).then_some(()),
    );
    let guild = guild.id;
    // A second channel, so there is an order to change.
    let extra = alice
        .ask(
            Command::CreateChannel {
                kind: thehideout_api::ChannelKind::Text,
                guild,
                name: "random".into(),
                section: None,
                icon: Default::default(),
                color: Default::default(),
            },
            |r| match r {
                Reply::ChannelCreated(c) => Some(c.clone()),
                _ => None,
            },
        )
        .expect("create channel");
    let channels = |peer: &mut Peer| {
        peer.ask(Command::LoadChannels { guild }, |r| match r {
            Reply::Channels { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("channels")
    };

    // Bob has no role: the layout is not his to change.
    let (_, layout) = channels(&mut bob);
    let mine = layout::normalized(&layout);
    let moved = layout::step_channel(&mine, extra.id, -1).expect("a real move");
    let refused = bob.ask(
        Command::UpdateLayout {
            guild,
            layout: moved.clone(),
        },
        |r| match r {
            Reply::LayoutSaved { result, .. } => Some(result.clone()),
            _ => None,
        },
    );
    assert_eq!(refused, Err(ApiError::PermissionDenied));

    // Alice makes the role with a colour and MANAGE_CHANNELS; bob's gateway carries the new list.
    bob.seen.clear();
    let (role, version) = alice
        .ask(
            Command::CreateRole {
                guild,
                name: "Builders".into(),
                color: PaletteColor::Blue,
                permissions: Permissions::MANAGE_CHANNELS,
            },
            |r| match r {
                Reply::RoleCreated { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("create role");
    assert_eq!(
        (role.name.as_str(), role.color, role.permissions),
        ("Builders", PaletteColor::Blue, Permissions::MANAGE_CHANNELS)
    );
    let announced = bob.wait("the roles on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::RolesUpdated {
            guild_id,
            roles,
            version: v,
        })) if *guild_id == guild && roles.iter().any(|x| x.id == role.id) => {
            Some((roles.clone(), *v))
        }
        _ => None,
    });
    assert_eq!(announced.1, version);

    // She hands it to bob; his gateway says his roles changed, and now the same move works.
    let member = alice
        .ask(
            Command::SetMemberRoles {
                guild,
                user: b.me.id,
                role_ids: vec![role.id],
            },
            |r| match r {
                Reply::MemberRolesSet { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("set member roles");
    assert_eq!(member.role_ids, vec![role.id]);
    bob.wait("bob's roles on his gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::MemberUpdated { member, .. }))
            if member.user.id == b.me.id && member.role_ids == [role.id] =>
        {
            Some(())
        }
        _ => None,
    });
    let access = bob
        .ask(Command::LoadAccess { guild }, |r| match r {
            Reply::Access { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("access");
    assert_eq!(access.version, version);
    assert_eq!(access.member_roles.get(&b.me.id), Some(&vec![role.id]));
    let saved = bob
        .ask(
            Command::UpdateLayout {
                guild,
                layout: moved,
            },
            |r| match r {
                Reply::LayoutSaved { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("bob may reorder now");
    assert_eq!(layout::channels_in(&saved, None).first(), Some(&extra.id));
    // He still can't manage roles: that was not in the role.
    let denied = bob.ask(
        Command::CreateRole {
            guild,
            name: "Mine".into(),
            color: PaletteColor::None,
            permissions: Permissions::NONE,
        },
        |r| match r {
            Reply::RoleCreated { result, .. } => Some(result.clone()),
            _ => None,
        },
    );
    assert_eq!(denied.map(|_| ()), Err(ApiError::PermissionDenied));

    // Role order: a second role, then a reorder on the current version, then the same one again (stale).
    let (guests, v2) = alice
        .ask(
            Command::CreateRole {
                guild,
                name: "Guests".into(),
                color: PaletteColor::Slate,
                permissions: Permissions::NONE,
            },
            |r| match r {
                Reply::RoleCreated { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("second role");
    let reorder = |peer: &mut Peer, expected_version: u64, ids: Vec<u64>| {
        peer.ask(
            Command::ReorderRoles {
                guild,
                expected_version,
                role_ids: ids.clone(),
            },
            |r| match r {
                Reply::RolesReordered { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
    };
    // New roles go to the bottom: Builders, Guests. Put Guests on top.
    let done = reorder(&mut alice, v2, vec![guests.id, role.id]).expect("reorder");
    assert_eq!(
        done.roles.iter().map(|r| r.id).collect::<Vec<_>>(),
        [guests.id, role.id]
    );
    assert!(done.version > v2);
    let stale = reorder(&mut alice, v2, vec![role.id, guests.id]);
    assert_eq!(
        stale.map(|_| ()),
        Err(ApiError::Conflict),
        "ABORTED is Conflict"
    );

    rt.block_on(server.cleanup());
    std::fs::remove_file(&alice.prefs).ok();
    std::fs::remove_file(&bob.prefs).ok();
}

/// An avatar uploaded by one user reaches the other's gateway as `UserUpdated`, and the picture
/// can be fetched by its key; a picked file is read through the backend; a server icon and a
/// removal go the same way.
#[test]
fn an_uploaded_avatar_reaches_a_second_client_and_can_be_fetched() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(Server::start_with_media());
    let mut alice = Peer::new(&server.url);
    let mut bob = Peer::new(&server.url);
    let a = alice.sign_up(&server, "alice");
    alice.initial();
    alice.connected();
    let guild = alice
        .ask(
            Command::CreateGuild {
                name: "friends".into(),
            },
            |r| match r {
                Reply::GuildCreated(g) => Some(g.clone()),
                _ => None,
            },
        )
        .expect("create guild");
    bob.sign_up(&server, "bob");
    bob.initial();
    bob.connected();
    bob.ask(
        Command::JoinGuild {
            code: guild.invite_code.clone(),
        },
        |r| matches!(r, Reply::GuildJoined(Ok(_))).then_some(()),
    );
    bob.seen.clear();

    // A picked file is read off-thread by the backend: its bytes and type come back as they were.
    let png = media::generated_png(3, 64);
    let path = std::env::temp_dir().join(format!(
        "thehideout-avatar-{:016x}.png",
        rand::rng().random::<u64>()
    ));
    std::fs::write(&path, &png).unwrap();
    let read = alice
        .ask(
            Command::ReadImage {
                path: path.clone(),
                purpose: backend::ImagePurpose::Avatar,
            },
            |r| match r {
                Reply::ImageRead { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("read the picked file");
    assert_eq!(read, (png.clone(), "image/png".to_owned()));
    std::fs::remove_file(&path).ok();
    let refused = alice.ask(
        Command::ReadImage {
            path: path.with_extension("txt"),
            purpose: backend::ImagePurpose::Avatar,
        },
        |r| match r {
            Reply::ImageRead { result, .. } => Some(result.clone()),
            _ => None,
        },
    );
    assert!(
        matches!(refused, Err(ApiError::InvalidArgument(_))),
        "only images are read"
    );

    // Upload: alice's reply has the key; bob's gateway announces the user with it.
    let user = alice
        .ask(
            Command::UploadAvatar {
                bytes: read.0,
                content_type: read.1,
            },
            |r| match r {
                Reply::AvatarSaved(u) => Some(u.clone()),
                _ => None,
            },
        )
        .expect("upload avatar");
    let key = user.avatar.clone().expect("the user has an avatar key now");
    let seen = bob.wait("alice's new avatar on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::UserUpdated(u)))
            if u.id == a.me.id && u.avatar.is_some() =>
        {
            Some(u.clone())
        }
        _ => None,
    });
    assert_eq!(seen.avatar.as_deref(), Some(key.as_str()));

    // Bob fetches the picture by key: real image bytes, resized by the server.
    let bytes = bob
        .ask(Command::FetchMedia { key: key.clone() }, |r| match r {
            Reply::Media { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("fetch the avatar");
    assert!(!bytes.is_empty());
    let cache = media::MediaCache::default();
    let side = cache
        .preview(bytes)
        .wait()
        .expect("the fetched bytes decode");
    assert_eq!(side[0], side[1], "the server stores it square");
    let unknown = bob.ask(
        Command::FetchMedia {
            key: "no-such-key".into(),
        },
        |r| match r {
            Reply::Media { result, .. } => Some(result.clone()),
            _ => None,
        },
    );
    assert_eq!(unknown, Err(ApiError::NotFound));

    // A guild icon, announced as GuildUpdated.
    let guild_after = alice
        .ask(
            Command::UploadGuildIcon {
                guild: guild.id,
                bytes: png.clone(),
                content_type: "image/png".into(),
            },
            |r| match r {
                Reply::GuildSaved { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("upload icon");
    let icon = guild_after.icon.clone().expect("the guild has an icon key");
    let seen = bob.wait("the new icon on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::GuildUpdated(g)))
            if g.id == guild.id && g.icon.is_some() =>
        {
            Some(g.clone())
        }
        _ => None,
    });
    assert_eq!(seen.icon.as_deref(), Some(icon.as_str()));
    // Bob is no manager: his try is refused.
    let denied = bob.ask(
        Command::UploadGuildIcon {
            guild: guild.id,
            bytes: png,
            content_type: "image/png".into(),
        },
        |r| match r {
            Reply::GuildSaved { result, .. } => Some(result.clone()),
            _ => None,
        },
    );
    assert_eq!(denied.map(|_| ()), Err(ApiError::PermissionDenied));

    // Removing the avatar is announced too.
    alice
        .ask(Command::RemoveAvatar, |r| match r {
            Reply::AvatarSaved(u) => Some(u.clone()),
            _ => None,
        })
        .expect("remove avatar");
    bob.wait("the removal on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::UserUpdated(u)))
            if u.id == a.me.id && u.avatar.is_none() =>
        {
            Some(())
        }
        _ => None,
    });

    rt.block_on(server.cleanup());
    std::fs::remove_file(&alice.prefs).ok();
    std::fs::remove_file(&bob.prefs).ok();
}

/// A file uploaded from disk with progress, sent with a message, reaches the second client as an
/// attachment, and downloads to the same bytes (streamed to a path, with progress).
#[test]
fn an_uploaded_file_reaches_a_second_client_and_downloads_identical_bytes() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(Server::start_with_media());
    let mut alice = Peer::new(&server.url);
    let mut bob = Peer::new(&server.url);
    alice.sign_up(&server, "alice");
    alice.initial();
    alice.connected();
    let guild = alice
        .ask(Command::CreateGuild { name: "friends".into() }, |r| match r {
            Reply::GuildCreated(g) => Some(g.clone()),
            _ => None,
        })
        .expect("create guild");
    let channel = alice
        .ask(Command::LoadChannels { guild: guild.id }, |r| match r {
            Reply::Channels { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("list channels")
        .0
        .first()
        .expect("a default channel")
        .id;
    bob.sign_up(&server, "bob");
    bob.initial();
    bob.connected();
    bob.ask(Command::JoinGuild { code: guild.invite_code.clone() }, |r| matches!(r, Reply::GuildJoined(Ok(_))).then_some(()));
    bob.seen.clear();

    // Bigger than one 64 KiB chunk, so progress has steps; a pattern, so a wrong byte shows.
    let data: Vec<u8> = (0..200_000u32).map(|i| (i.wrapping_mul(31) % 251) as u8).collect();
    let dir = std::env::temp_dir().join(format!("thehideout-files-{:016x}", rand::rng().random::<u64>()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("blob.bin");
    std::fs::write(&source, &data).unwrap();

    let pending = alice
        .ask(
            Command::UploadFile {
                channel,
                id: 7,
                path: source.clone(),
                filename: "blob.bin".into(),
                content_type: "application/octet-stream".into(),
                size: data.len() as u64,
            },
            |r| match r {
                Reply::Uploaded { id: 7, result } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("the upload is accepted");
    assert_eq!((pending.filename.as_str(), pending.size), ("blob.bin", data.len() as u64));
    let steps: Vec<u64> = alice
        .seen
        .iter()
        .filter_map(|r| match r {
            Reply::UploadProgress { id: 7, sent, total } => {
                assert_eq!(*total, data.len() as u64);
                Some(*sent)
            }
            _ => None,
        })
        .collect();
    assert!(steps.len() >= 2 && steps.windows(2).all(|w| w[0] < w[1]), "progress moved up in steps: {steps:?}");
    assert_eq!(steps.last().copied(), Some(data.len() as u64), "and reached the whole file");

    // Sent with a message: alice's reply and bob's gateway both carry the attachment.
    alice.ask(
        Command::SendWith { channel, content: "the blob".into(), nonce: "n1".into(), attachments: vec![pending.id] },
        |r| match r {
            Reply::Sent { result, .. } => Some(result.clone().expect("sent")),
            _ => None,
        },
    );
    let message = bob.wait("the message with its file on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::MessageCreated(m))) if !m.attachments.is_empty() => Some(m.clone()),
        _ => None,
    });
    assert_eq!(message.content, "the blob");
    let att = &message.attachments[0];
    assert_eq!((att.filename.as_str(), att.size, att.removed), ("blob.bin", data.len() as u64, false));
    let key = att.file.clone().expect("the file has a key");

    // Bob downloads it: streamed to a path, progress reported, identical bytes.
    let out = dir.join("downloaded.bin");
    let written = bob
        .ask(Command::Download { key: key.clone(), path: out.clone() }, |r| match r {
            Reply::Downloaded { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("download");
    assert_eq!(written, data.len() as u64);
    assert_eq!(std::fs::read(&out).unwrap(), data, "the same bytes came back");
    assert!(bob.seen.iter().any(|r| matches!(r, Reply::DownloadProgress { received, .. } if *received > 0)), "progress was reported");
    // An unknown key fails and leaves no file.
    let gone = dir.join("gone.bin");
    let missing = bob.ask(Command::Download { key: "no-such-key".into(), path: gone.clone() }, |r| match r {
        Reply::Downloaded { result, .. } => Some(result.clone()),
        _ => None,
    });
    assert_eq!(missing, Err(ApiError::NotFound));
    assert!(!gone.exists());

    rt.block_on(server.cleanup());
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&alice.prefs).ok();
    std::fs::remove_file(&bob.prefs).ok();
}

/// Custom emojis, reactions and an emoji channel icon through the real server: an upload reaches
/// the second client as `EmojisUpdated` and its picture can be fetched; one user's reaction is seen
/// by the other; the channel's `icon_ref` can be a custom emoji, and deleting the emoji says so.
#[test]
fn emojis_reactions_and_an_emoji_channel_icon_reach_a_second_client() {
    use thehideout_api::{ChannelIconRef, ReactionEmoji};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(Server::start_with_media());
    let mut alice = Peer::new(&server.url);
    let mut bob = Peer::new(&server.url);
    let a = alice.sign_up(&server, "alice");
    alice.initial();
    alice.connected();
    let guild = alice
        .ask(
            Command::CreateGuild {
                name: "friends".into(),
            },
            |r| match r {
                Reply::GuildCreated(g) => Some(g.clone()),
                _ => None,
            },
        )
        .expect("create guild");
    let b = bob.sign_up(&server, "bob");
    bob.initial();
    bob.connected();
    bob.ask(
        Command::JoinGuild {
            code: guild.invite_code.clone(),
        },
        |r| matches!(r, Reply::GuildJoined(Ok(_))).then_some(()),
    );
    let g = guild.id;
    bob.seen.clear();

    // Nothing yet. Bob (a plain member) may not add one: MANAGE_EMOJIS is the owner's here.
    let none = alice
        .ask(Command::LoadEmojis { guild: g }, |r| match r {
            Reply::Emojis { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("list emojis");
    assert!(none.is_empty());
    let png = media::generated_emoji(5, 64);
    let denied = bob.ask(
        Command::CreateEmoji {
            guild: g,
            name: "nope".into(),
            bytes: png.clone(),
            content_type: "image/png".into(),
        },
        |r| match r {
            Reply::EmojiSaved { result, .. } => Some(result.clone()),
            _ => None,
        },
    );
    assert_eq!(denied.map(|_| ()), Err(ApiError::PermissionDenied));
    // A bad name is the server's `INVALID_ARGUMENT` too, whatever the client checks first.
    let bad = alice.ask(
        Command::CreateEmoji {
            guild: g,
            name: "Bad Name".into(),
            bytes: png.clone(),
            content_type: "image/png".into(),
        },
        |r| match r {
            Reply::EmojiSaved { result, .. } => Some(result.clone()),
            _ => None,
        },
    );
    assert!(matches!(bad, Err(ApiError::InvalidArgument(_))));

    // Alice uploads a static one and an animated one; bob's gateway carries the full list each time.
    let cat = alice
        .ask(
            Command::CreateEmoji {
                guild: g,
                name: "smug_cat".into(),
                bytes: png.clone(),
                content_type: "image/png".into(),
            },
            |r| match r {
                Reply::EmojiSaved { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("create emoji");
    assert_eq!(
        (
            cat.name.as_str(),
            cat.animated,
            cat.guild_id,
            cat.creator_id
        ),
        ("smug_cat", false, g, a.me.id)
    );
    let seen = bob.wait("the new emoji on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::EmojisUpdated { guild_id, emojis }))
            if *guild_id == g && emojis.iter().any(|e| e.id == cat.id) =>
        {
            Some(emojis.clone())
        }
        _ => None,
    });
    assert_eq!(seen.len(), 1);
    let gif = media::generated_animated_emoji(6, 64, 6);
    let parrot = alice
        .ask(
            Command::CreateEmoji {
                guild: g,
                name: "party_parrot".into(),
                bytes: gif,
                content_type: "image/gif".into(),
            },
            |r| match r {
                Reply::EmojiSaved { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("create the animated emoji");
    assert!(parrot.animated, "a GIF with several frames is animated");
    bob.wait("the second emoji on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::EmojisUpdated { emojis, .. }))
            if emojis.len() == 2 =>
        {
            Some(())
        }
        _ => None,
    });
    // A taken name is refused.
    let dup = alice.ask(
        Command::CreateEmoji {
            guild: g,
            name: "smug_cat".into(),
            bytes: png.clone(),
            content_type: "image/png".into(),
        },
        |r| match r {
            Reply::EmojiSaved { result, .. } => Some(result.clone()),
            _ => None,
        },
    );
    assert_eq!(dup.map(|_| ()), Err(ApiError::AlreadyExists));

    // Bob lists them and fetches the picture by its key: it decodes, and the animated one is a GIF.
    let listed = bob
        .ask(Command::LoadEmojis { guild: g }, |r| match r {
            Reply::Emojis { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("bob lists emojis");
    assert_eq!(
        listed.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["party_parrot", "smug_cat"],
        "by name"
    );
    let fetch = |peer: &mut Peer, key: &str| {
        peer.ask(
            Command::FetchMedia {
                key: key.to_owned(),
            },
            |r| match r {
                Reply::Media { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("fetch the emoji")
    };
    let bytes = fetch(&mut bob, &cat.image);
    let cache = media::MediaCache::default();
    assert!(
        cache.preview(bytes).wait().is_ok(),
        "the fetched picture decodes"
    );
    let moving = fetch(&mut bob, &parrot.image);
    assert!(
        moving.starts_with(b"GIF8"),
        "animated emojis are served as GIFs"
    );
    assert!(
        media::first_frame(&moving).is_some(),
        "so the client can make its still"
    );

    // A rename and its event.
    let renamed = alice
        .ask(
            Command::RenameEmoji {
                guild: g,
                emoji: cat.id,
                name: "smug_dog".into(),
            },
            |r| match r {
                Reply::EmojiSaved { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("rename");
    assert_eq!(renamed.name, "smug_dog");
    bob.wait("the rename on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::EmojisUpdated { emojis, .. }))
            if emojis
                .iter()
                .any(|e| e.id == cat.id && e.name == "smug_dog") =>
        {
            Some(())
        }
        _ => None,
    });

    // A message, and reactions: alice's thumbs-up and a custom one reach bob; bob adds his own.
    let (channels, _) = alice
        .ask(Command::LoadChannels { guild: g }, |r| match r {
            Reply::Channels { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("channels");
    let channel = channels[0].id;
    let message = alice
        .ask(
            Command::Send {
                channel,
                content: "look <:smug_dog:1>".replace('1', &cat.id.to_string()),
                nonce: "n1".into(),
            },
            |r| match r {
                Reply::Sent { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("send")
        .id;
    bob.seen.clear();
    let thumbs = ReactionEmoji::Unicode("\u{1f44d}".into());
    let react = |peer: &mut Peer, emoji: ReactionEmoji, add: bool| {
        let cmd = if add {
            Command::AddReaction {
                channel,
                message,
                emoji,
            }
        } else {
            Command::RemoveReaction {
                channel,
                message,
                emoji,
            }
        };
        peer.ask(cmd, |r| match r {
            Reply::ReactionSaved { result, .. } => Some(result.clone()),
            _ => None,
        })
    };
    react(&mut alice, thumbs.clone(), true).expect("add");
    react(&mut alice, ReactionEmoji::Custom(cat.id), true).expect("add custom");
    let user = bob.wait("alice's reaction on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::ReactionAdded {
            message_id,
            emoji,
            user_id,
            ..
        })) if *message_id == message && *emoji == thumbs => Some(*user_id),
        _ => None,
    });
    assert_eq!(user, a.me.id, "the event carries who reacted");
    // Idempotent: the same again changes nothing and is not an error.
    react(&mut alice, thumbs.clone(), true).expect("idempotent add");
    react(&mut bob, thumbs.clone(), true).expect("bob adds the same emoji");
    let page = bob
        .ask(
            Command::LoadMessages {
                channel,
                before: None,
            },
            |r| match r {
                Reply::Messages { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("messages");
    let m = page.items.iter().find(|m| m.id == message).unwrap();
    let t = m.reactions.iter().find(|r| r.emoji == thumbs).unwrap();
    assert_eq!(
        (t.count, t.me),
        (2, true),
        "two reacted, and bob is one of them"
    );
    let c = m
        .reactions
        .iter()
        .find(|r| r.emoji == ReactionEmoji::Custom(cat.id))
        .unwrap();
    assert_eq!((c.count, c.me), (1, false));
    alice.wait("bob's reaction on alice's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::ReactionAdded { emoji, user_id, .. }))
            if *emoji == thumbs && *user_id == b.me.id =>
        {
            Some(())
        }
        _ => None,
    });
    react(&mut bob, thumbs.clone(), false).expect("remove");
    alice.wait("bob taking it back", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::ReactionRemoved { emoji, user_id, .. }))
            if *emoji == thumbs && *user_id == b.me.id =>
        {
            Some(())
        }
        _ => None,
    });
    // A custom emoji from nowhere is refused.
    let foreign = react(&mut bob, ReactionEmoji::Custom(1), true);
    assert!(
        matches!(
            foreign,
            Err(ApiError::InvalidArgument(_)) | Err(ApiError::NotFound)
        ),
        "{foreign:?}"
    );

    // The channel's icon is the custom emoji; bob sees the update; it can go back to the default.
    let saved = alice
        .ask(
            Command::UpdateChannel {
                guild: g,
                channel,
                name: None,
                icon: Some(ChannelIconRef::Custom(cat.id)),
                color: None,
                topic: None,
            },
            |r| match r {
                Reply::ChannelSaved { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("set the icon");
    assert_eq!(saved.icon_ref, ChannelIconRef::Custom(cat.id));
    bob.wait("the new icon on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::ChannelUpdated(c)))
            if c.id == channel && c.icon_ref == ChannelIconRef::Custom(cat.id) =>
        {
            Some(())
        }
        _ => None,
    });
    let uni = alice
        .ask(
            Command::UpdateChannel {
                guild: g,
                channel,
                name: None,
                icon: Some(ChannelIconRef::Unicode("\u{1f680}".into())),
                color: None,
                topic: None,
            },
            |r| match r {
                Reply::ChannelSaved { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("set a Unicode icon");
    assert_eq!(uni.icon_ref, ChannelIconRef::Unicode("\u{1f680}".into()));
    let created = alice
        .ask(
            Command::CreateChannel {
                kind: thehideout_api::ChannelKind::Text,
                guild: g,
                name: "party".into(),
                section: None,
                icon: ChannelIconRef::Custom(parrot.id),
                color: Default::default(),
            },
            |r| match r {
                Reply::ChannelCreated(c) => Some(c.clone()),
                _ => None,
            },
        )
        .expect("a channel created with an emoji icon");
    assert_eq!(created.icon_ref, ChannelIconRef::Custom(parrot.id));
    // An emoji of another guild (here: one that doesn't exist) is not a valid icon.
    let bogus = alice.ask(
        Command::UpdateChannel {
            guild: g,
            channel,
            name: None,
            icon: Some(ChannelIconRef::Custom(1)),
            color: None,
            topic: None,
        },
        |r| match r {
            Reply::ChannelSaved { result, .. } => Some(result.clone()),
            _ => None,
        },
    );
    assert!(
        matches!(bogus, Err(ApiError::InvalidArgument(_))),
        "{bogus:?}"
    );

    // Delete: bob's gateway carries the shorter list.
    alice
        .ask(
            Command::DeleteEmoji {
                guild: g,
                emoji: cat.id,
            },
            |r| match r {
                Reply::EmojiDeleted { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("delete");
    bob.wait("the deletion on bob's gateway", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::EmojisUpdated { emojis, .. }))
            if emojis.len() == 1 && emojis[0].id == parrot.id =>
        {
            Some(())
        }
        _ => None,
    });

    rt.block_on(server.cleanup());
    std::fs::remove_file(&alice.prefs).ok();
    std::fs::remove_file(&bob.prefs).ok();
}

/// An invite made on the server (as the `invite` CLI does) is looked up and accepted through
/// the backend, which signs the new account in on a named device. Turning on an authenticator
/// makes the next sign-in (another device) stop at the second factor, which a code from the
/// authenticator completes; the first device then lists both sessions, itself as the current one.
#[test]
fn an_invite_then_two_factor_then_a_second_device() {
    use thehideout_api::{InviteStatus, SecondFactorMethod};
    use thehideout_server::services::invite::{NewInvite, create};
    use totp_rs::{Algorithm, Secret, TOTP};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(Server::start());
    let (_, _token, code) = rt
        .block_on(create(
            &server.state,
            NewInvite {
                email: "erin@example.test".into(),
                inviter_id: None,
                guild_id: None,
                guild_ids: Vec::new(),
                make_admin: false,
            },
        ))
        .expect("create an invite");

    // Looked up, then accepted: the account is signed in and saved.
    let mut desk = Peer::new(&server.url);
    let preview = desk
        .ask(Command::LookupInvite { code: code.to_lowercase() }, |r| match r {
            Reply::InviteLookup(p) => Some(p.clone()),
            _ => None,
        })
        .expect("look the invite up (the code's case is forgiven)");
    assert_eq!(preview.status, InviteStatus::Pending);
    assert_eq!(preview.masked_email, "e***@example.test");
    let erin = desk
        .ask(
            Command::AcceptInvite {
                code: code.clone(),
                username: "erin".into(),
                password: Some(PASSWORD.into()),
            },
            |r| match r {
                Reply::Auth(a) => Some(a.clone()),
                _ => None,
            },
        )
        .expect("accept the invite");
    assert_eq!(erin.me.username, "erin");
    assert_eq!(erin.me.email.as_deref(), Some("erin@example.test"));
    assert!(desk.saved().is_some(), "the new session is saved");
    // The invite is spent.
    let again = desk.ask(
        Command::AcceptInvite { code, username: "erin2".into(), password: Some(PASSWORD.into()) },
        |r| match r {
            Reply::Auth(a) => Some(a.clone()),
            _ => None,
        },
    );
    assert!(matches!(again, Err(ApiError::Precondition(_))), "a used invite: {again:?}");

    // Relaunched, the saved session resumes; there the authenticator is turned on.
    let mut desk = Peer::with_prefs(desk.prefs.clone());
    desk.backend.set_server(&server.url);
    let resumed = desk.ask(Command::Resume, |r| match r {
        Reply::Resume(r) => Some(r.clone()),
        _ => None,
    });
    assert!(matches!(resumed, Ok(Some(_))), "the saved session resumes: {resumed:?}");
    let setup = desk
        .ask(Command::BeginTotpSetup, |r| match r {
            Reply::TotpSetup(s) => Some(s.clone()),
            _ => None,
        })
        .expect("begin the authenticator setup");
    assert!(setup.otpauth_uri.starts_with("otpauth://totp/"));
    let totp = TOTP::new_unchecked(
        Algorithm::SHA1,
        6,
        1,
        30,
        Secret::Encoded(setup.secret.clone()).to_bytes().expect("a base32 secret"),
        None,
        String::new(),
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let codes = desk
        .ask(Command::ConfirmTotp { code: totp.generate(now) }, |r| match r {
            Reply::RecoveryCodes { regenerated: false, result } => Some(result.clone()),
            _ => None,
        })
        .expect("confirm the authenticator");
    assert_eq!(codes.len(), 10, "ten recovery codes");

    // A second device: the password is not enough any more.
    let mut laptop = Peer::new(&server.url);
    let (challenge, methods) = laptop.ask(
        Command::Login { username: "erin@example.test".into(), password: PASSWORD.into() },
        |r| match r {
            Reply::SecondFactor { challenge, methods } => Some((challenge.clone(), methods.clone())),
            Reply::Auth(a) => panic!("signed in without the second factor: {a:?}"),
            _ => None,
        },
    );
    assert!(methods.contains(&SecondFactorMethod::Totp) && methods.contains(&SecondFactorMethod::RecoveryCode));
    // A wrong code is refused and leaves the challenge open...
    let wrong = laptop.ask(
        Command::CompleteSecondFactor { challenge: challenge.clone(), code: "000000".into(), recovery: false },
        |r| match r {
            Reply::Auth(a) => Some(a.clone()),
            _ => None,
        },
    );
    assert_eq!(wrong.map(|s| s.me.id), Err(ApiError::WrongCode { attempts_remaining: 4 }));
    // ...and the next step's code (each step is accepted once; the setup used this one) completes it.
    let second = laptop
        .ask(
            Command::CompleteSecondFactor { challenge, code: totp.generate(now + 30), recovery: false },
            |r| match r {
                Reply::Auth(a) => Some(a.clone()),
                _ => None,
            },
        )
        .expect("complete the second factor");
    assert_eq!(second.me.id, erin.me.id);

    // The first device sees both sessions, itself as the current one, and the authenticator on.
    let info = desk
        .ask(Command::LoadSecurity, |r| match r {
            Reply::Security(s) => Some(s.clone()),
            _ => None,
        })
        .expect("the security lists");
    assert_eq!(info.sessions.len(), 2, "two devices: {:?}", info.sessions);
    assert_eq!(info.sessions.iter().filter(|s| s.current).count(), 1);
    let label = backend::device_label();
    assert!(info.sessions.iter().all(|s| s.device_name == label), "named after this computer: {:?}", info.sessions);
    assert!(info.summary.totp_enabled && info.summary.has_password && info.summary.email_verified);
    assert_eq!((info.summary.recovery_codes_remaining, info.summary.passkey_count), (10, 0));
    assert!(info.passkeys.is_empty());

    // Signing the other device out leaves one.
    let other = info.sessions.iter().find(|s| !s.current).unwrap().id;
    desk.ask(Command::RevokeSession { id: other }, |r| match r {
        Reply::SessionRevoked { id, result } if *id == other => Some(result.clone()),
        _ => None,
    })
    .expect("revoke the other session");
    let info = desk
        .ask(Command::LoadSecurity, |r| match r {
            Reply::Security(s) => Some(s.clone()),
            _ => None,
        })
        .expect("the security lists again");
    assert_eq!(info.sessions.len(), 1);

    rt.block_on(server.cleanup());
}

/// A plain HTTP/1.1 request (`body`: a POST), as a browser makes it: the status and the body.
async fn http(url: &str, body: Option<&str>) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rest = url.strip_prefix("http://").expect("an http URL");
    let (host, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, "/".to_owned()),
    };
    let mut stream = tokio::net::TcpStream::connect(host).await.expect("connect");
    let (method, body) = match body {
        Some(b) => ("POST", b),
        None => ("GET", ""),
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    stream.read_to_end(&mut out).await.unwrap();
    let text = String::from_utf8_lossy(&out).into_owned();
    let status = text.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, text.split_once("\r\n\r\n").map(|(_, b)| b.to_owned()).unwrap_or_default())
}

/// What the system browser does with a ceremony page the backend asked to open: fetch the
/// options, run WebAuthn with `key`, post the result, then follow the page's redirect to the
/// app's loopback address. Returns what the loopback answered.
async fn browser_ceremony(
    server: &Server,
    ceremony_url: &str,
    key: &mut webauthn_authenticator_rs::softpasskey::SoftPasskey,
) -> (u16, String) {
    use webauthn_authenticator_rs::WebauthnAuthenticator;
    let id = ceremony_url.split("ceremony=").nth(1).expect("a ceremony id");
    let (status, cfg) = http(&format!("{}/auth/passkey/options?ceremony={id}", server.url), None).await;
    assert_eq!(status, 200, "{cfg}");
    let cfg: serde_json::Value = serde_json::from_str(&cfg).unwrap();
    let origin = server.state.public_url.clone();
    let options = cfg["options"].clone();
    let credential = if cfg["kind"] == "register" {
        serde_json::to_string(&key.do_registration(origin, serde_json::from_value(options).unwrap()).unwrap())
    } else {
        serde_json::to_string(&key.do_authentication(origin, serde_json::from_value(options).unwrap()).unwrap())
    }
    .unwrap();
    let (status, _) = http(&format!("{}/auth/passkey/result?ceremony={id}", server.url), Some(&credential)).await;
    assert_eq!(status, 204);
    let redirect = cfg["redirect"].as_str().expect("the backend asked for the loopback redirect");
    assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/done"), "{redirect}");
    http(redirect, None).await
}

/// A passkey (added through the browser ceremony, which redirects to the backend's loopback
/// listener) answers the second factor of a sign-in on another device: the challenge's own
/// ceremony, not a fresh passkey sign-in. Wrong codes come back as `WrongCode` with the server's
/// count and never end a session; the Security lists come from `GetSecurity`, with no TOTP probe.
#[test]
fn a_passkey_answers_the_second_factor_and_wrong_codes_keep_the_session() {
    use thehideout_api::SecondFactorMethod;
    use totp_rs::{Algorithm, Secret, TOTP};
    use webauthn_authenticator_rs::softpasskey::SoftPasskey;

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(Server::start());
    let mut desk = Peer::new(&server.url);
    let pia = desk.sign_up(&server, "pia");
    let mut key = SoftPasskey::new(true);

    // A passkey, through the ceremony page: the browser lands on the app's loopback page.
    desk.backend.submit(Command::BeginPasskeyRegistration { name: "laptop".into() });
    let url = desk
        .wait("the registration page", |r| match r {
            Reply::PasskeyCeremony(u) => Some(u.clone()),
            _ => None,
        })
        .expect("a ceremony page");
    let started = Instant::now();
    let (status, page) = rt.block_on(browser_ceremony(&server, &url, &mut key));
    assert_eq!(status, 200);
    assert!(page.contains("close this tab"), "{page}");
    let added = desk
        .wait("the new passkey", |r| match r {
            Reply::PasskeyAdded(p) => Some(p.clone()),
            _ => None,
        })
        .expect("the passkey is added");
    assert_eq!(added.name, "laptop");
    assert!(started.elapsed() < Duration::from_secs(5));

    // The authenticator, then a wrong signed-in code: a `WrongCode`, and the session lives on.
    let setup = desk
        .ask(Command::BeginTotpSetup, |r| match r {
            Reply::TotpSetup(s) => Some(s.clone()),
            _ => None,
        })
        .expect("begin the authenticator setup");
    let totp = TOTP::new_unchecked(
        Algorithm::SHA1,
        6,
        1,
        30,
        Secret::Encoded(setup.secret.clone()).to_bytes().expect("a base32 secret"),
        None,
        String::new(),
    );
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let wrong = desk.ask(Command::ConfirmTotp { code: "000000".into() }, |r| match r {
        Reply::RecoveryCodes { result, .. } => Some(result.clone()),
        _ => None,
    });
    assert_eq!(wrong, Err(ApiError::WrongCode { attempts_remaining: 9 }), "ten checks an hour");
    desk.ask(Command::ConfirmTotp { code: totp.generate(now) }, |r| match r {
        Reply::RecoveryCodes { result, .. } => Some(result.clone()),
        _ => None,
    })
    .expect("confirm the authenticator");
    let info = desk
        .ask(Command::LoadSecurity, |r| match r {
            Reply::Security(s) => Some(s.clone()),
            _ => None,
        })
        .expect("the security lists after a wrong code: still signed in");
    assert!(info.summary.totp_enabled);
    assert_eq!((info.summary.passkey_count, info.summary.recovery_codes_remaining), (1, 10));
    assert!(desk.seen.iter().all(|r| !matches!(r, Reply::Gateway(GatewayUpdate::AuthExpired))));

    // Another device: the password stops at the second factor, which offers the passkey.
    let mut laptop = Peer::new(&server.url);
    let (challenge, methods) = laptop.ask(
        Command::Login { username: "pia".into(), password: PASSWORD.into() },
        |r| match r {
            Reply::SecondFactor { challenge, methods } => Some((challenge.clone(), methods.clone())),
            Reply::Auth(a) => panic!("signed in without the second factor: {a:?}"),
            _ => None,
        },
    );
    assert!(methods.contains(&SecondFactorMethod::Passkey));
    let wrong = laptop.ask(
        Command::CompleteSecondFactor { challenge: challenge.clone(), code: "000000".into(), recovery: false },
        |r| match r {
            Reply::Auth(a) => Some(a.clone()),
            _ => None,
        },
    );
    assert_eq!(wrong.map(|s| s.me.id), Err(ApiError::WrongCode { attempts_remaining: 4 }));
    laptop.backend.submit(Command::BeginSecondFactorPasskey { challenge: challenge.clone() });
    let url = laptop
        .wait("the second-factor page", |r| match r {
            Reply::PasskeyCeremony(u) => Some(u.clone()),
            _ => None,
        })
        .expect("a ceremony page for the challenge");
    let (status, _) = rt.block_on(browser_ceremony(&server, &url, &mut key));
    assert_eq!(status, 200);
    let signed = laptop
        .wait("the session", |r| match r {
            Reply::Auth(a) => Some(a.clone()),
            _ => None,
        })
        .expect("the passkey completes the sign-in");
    assert_eq!(signed.me.id, pia.me.id);
    assert!(laptop.saved().is_some(), "the new session is saved");
    // The challenge is spent.
    let again = laptop.ask(
        Command::CompleteSecondFactor { challenge, code: totp.generate(now + 30), recovery: false },
        |r| match r {
            Reply::Auth(a) => Some(a.clone()),
            _ => None,
        },
    );
    assert_eq!(again.map(|s| s.me.id), Err(ApiError::CodeExpired));

    rt.block_on(server.cleanup());
}

#[test]
fn voice_joins_moderation_and_a_move_reach_a_second_client() {
    use thehideout_api::{ChannelKind, VoiceJoin};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(Server::start_with_voice());
    let mut alice = Peer::new(&server.url);
    let mut bob = Peer::new(&server.url);
    alice.sign_up(&server, "alice");
    alice.initial();
    alice.connected();
    let guild = alice
        .ask(Command::CreateGuild { name: "friends".into() }, |r| match r {
            Reply::GuildCreated(g) => Some(g.clone()),
            _ => None,
        })
        .expect("create guild");
    let b = bob.sign_up(&server, "bob");
    bob.initial();
    bob.connected();
    bob.ask(Command::JoinGuild { code: guild.invite_code.clone() }, |r| {
        matches!(r, Reply::GuildJoined(Ok(_))).then_some(())
    });
    let guild = guild.id;
    let mut voice_channel = |name: &str| {
        alice
            .ask(
                Command::CreateChannel {
                    guild,
                    name: name.into(),
                    kind: ChannelKind::Voice,
                    section: None,
                    icon: Default::default(),
                    color: Default::default(),
                },
                |r| match r {
                    Reply::ChannelCreated(c) => Some(c.clone()),
                    _ => None,
                },
            )
            .expect("create voice channel")
    };
    let (lounge, games) = (voice_channel("lounge"), voice_channel("games"));
    assert_eq!(lounge.kind, ChannelKind::Voice);

    // Joining answers with the LiveKit url and token, and bob's gateway shows alice in the channel.
    bob.seen.clear();
    let joined = alice
        .ask(Command::JoinVoice { channel: lounge.id }, |r| match r {
            Reply::VoiceJoined { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("join voice");
    let VoiceJoin { url, token, channel_id } = joined;
    assert_eq!((url.as_str(), channel_id), ("ws://127.0.0.1:7880", lounge.id));
    assert!(!token.is_empty());
    let state = bob.wait("alice in voice", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::VoiceStateUpdate { state: Some(s), .. })) => Some(s.clone()),
        _ => None,
    });
    assert_eq!(state.channel_id, lounge.id);

    // Alice's own flags.
    alice.ask(Command::UpdateVoiceState { self_mute: true, self_deaf: false }, |r| {
        matches!(r, Reply::VoiceFlags(Ok(s)) if s.self_mute).then_some(())
    });

    // Bob joins, alice (the owner) server-mutes him and moves him; bob is told to rejoin.
    bob.ask(Command::JoinVoice { channel: lounge.id }, |r| matches!(r, Reply::VoiceJoined { result: Ok(_), .. }).then_some(()));
    alice.ask(Command::ServerMuteMember { guild, user: b.me.id, muted: true }, |r| {
        matches!(r, Reply::VoiceModerated { result: Ok(()), .. }).then_some(())
    });
    bob.wait("the server mute", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::VoiceStateUpdate { state: Some(s), .. })) if s.server_mute => Some(()),
        _ => None,
    });
    alice.ask(Command::MoveMember { guild, user: b.me.id, channel: games.id }, |r| {
        matches!(r, Reply::VoiceModerated { result: Ok(()), .. }).then_some(())
    });
    bob.wait("voice_moved", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::VoiceMoved { channel_id, .. })) if *channel_id == games.id => Some(()),
        _ => None,
    });

    // Bob (no rights) can't moderate; alice disconnects him.
    let refused = bob.ask(Command::ServerMuteMember { guild, user: a_id(&mut alice), muted: true }, |r| match r {
        Reply::VoiceModerated { result, .. } => Some(result.clone()),
        _ => None,
    });
    assert_eq!(refused, Err(ApiError::PermissionDenied));
    alice.ask(Command::DisconnectMember { guild, user: b.me.id }, |r| {
        matches!(r, Reply::VoiceModerated { result: Ok(()), .. }).then_some(())
    });
    alice.ask(Command::LeaveVoice, |r| matches!(r, Reply::VoiceLeft(Ok(()))).then_some(()));
    rt.block_on(server.cleanup());
}

/// Alice's user id, from her session.
fn a_id(alice: &mut Peer) -> u64 {
    alice
        .ask(Command::LoadInitial, |r| match r {
            Reply::Initial(i) => Some(i.clone()),
            _ => None,
        })
        .expect("initial load")
        .me
        .id
}

#[test]
fn moderation_and_structure_commands_reach_the_real_server_and_a_second_client() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(Server::start());
    let (mut alice, mut bob) = (Peer::new(&server.url), Peer::new(&server.url));
    alice.sign_up(&server, "alice");
    let b = bob.sign_up(&server, "bob");
    alice.initial();
    alice.connected();
    bob.initial();
    bob.connected();
    let guild = alice
        .ask(Command::CreateGuild { name: "friends".into() }, |r| match r {
            Reply::GuildCreated(g) => Some(g.clone()),
            _ => None,
        })
        .expect("create guild");
    bob.ask(Command::JoinGuild { code: guild.invite_code.clone() }, |r| match r {
        Reply::GuildJoined(g) => Some(g.clone()),
        _ => None,
    })
    .expect("join");
    let general = alice
        .ask(Command::LoadChannels { guild: guild.id }, |r| match r {
            Reply::Channels { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("channels")
        .0[0]
        .id;

    // Edit your own message, and a moderator deletes it; the author's client hears.
    let sent = bob
        .ask(Command::Send { channel: general, content: "first".into(), nonce: "n1".into() }, |r| match r {
            Reply::Sent { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("send");
    let edited = bob
        .ask(Command::EditMessage { channel: general, message: sent.id, content: "second".into() }, |r| match r {
            Reply::MessageEdited { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("edit");
    assert_eq!((edited.content.as_str(), edited.edited_at.is_some()), ("second", true));
    alice
        .ask(Command::DeleteMessage { channel: general, message: sent.id }, |r| match r {
            Reply::MessageDeleted { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("delete");
    bob.wait("message_deleted", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::MessageDeleted { message_id, .. })) if *message_id == sent.id => Some(()),
        _ => None,
    });

    // Kick: bob's stream says the guild is gone; he can rejoin. Ban: he cannot, until unbanned.
    alice
        .ask(Command::KickMember { guild: guild.id, user: b.me.id }, |r| match r {
            Reply::Kicked { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("kick");
    bob.wait("guild_removed", |r| match r {
        Reply::Gateway(GatewayUpdate::Event(Event::GuildRemoved { guild_id })) if *guild_id == guild.id => Some(()),
        _ => None,
    });
    let join = |bob: &mut Peer, code: &str| {
        bob.ask(Command::JoinGuild { code: code.into() }, |r| match r {
            Reply::GuildJoined(g) => Some(g.clone()),
            _ => None,
        })
    };
    join(&mut bob, &guild.invite_code).expect("kicked members may rejoin");
    alice
        .ask(
            Command::BanMember { guild: guild.id, user: b.me.id, reason: "spam".into(), delete_seconds: None },
            |r| match r {
                Reply::Banned { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("ban");
    assert_eq!(join(&mut bob, &guild.invite_code).unwrap_err(), ApiError::PermissionDenied);
    let bans = alice
        .ask(Command::LoadBans { guild: guild.id }, |r| match r {
            Reply::Bans { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("bans");
    assert_eq!((bans[0].user.id, bans[0].reason.as_str()), (b.me.id, "spam"));
    alice
        .ask(Command::UnbanMember { guild: guild.id, user: b.me.id }, |r| match r {
            Reply::Unbanned { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("unban");

    // The new code works, the old one does not; then hand the guild over and delete it.
    let code = alice
        .ask(Command::RotateInviteCode { guild: guild.id }, |r| match r {
            Reply::InviteRotated { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("rotate");
    assert_eq!(join(&mut bob, &guild.invite_code).unwrap_err(), ApiError::NotFound);
    join(&mut bob, &code).expect("the new code works");
    let handed = alice
        .ask(
            Command::TransferOwnership { guild: guild.id, user: b.me.id, confirm_name: "friends".into() },
            |r| match r {
                Reply::OwnershipTransferred { result, .. } => Some(result.clone()),
                _ => None,
            },
        )
        .expect("transfer");
    assert_eq!(handed.owner_id, b.me.id);
    alice
        .ask(Command::LeaveGuild { guild: guild.id }, |r| match r {
            Reply::GuildLeft { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("leave");
    bob.ask(Command::DeleteGuild { guild: guild.id, confirm_name: "friends".into() }, |r| match r {
        Reply::GuildDeleted { result, .. } => Some(result.clone()),
        _ => None,
    })
    .expect("delete the guild");
    rt.block_on(server.cleanup());
}
