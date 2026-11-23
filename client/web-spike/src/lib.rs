//! Phase 0 spike: log in from the browser, make a unary call, and show live gateway events.
//!
//! Page query: `?user=<name or email>&pass=<password>` logs in; add `&invite=<code>` to create
//! the account from an invite first. The API is the page's own origin.

#[cfg(target_arch = "wasm32")]
mod web {
    use rune::tok::{self, type_};
    use rune::{App, AppSpec, Cx, El, ScrollSpec, col, el, text};
    use thehideout_api::{Client, Event, Gateway, GatewayUpdate, LoginOutcome};

    #[derive(Default)]
    struct State {
        log: Vec<String>,
    }

    type Handle = rune::AppHandle<State>;

    fn line(handle: &Handle, msg: impl Into<String>) {
        let msg = msg.into();
        web_sys::console::log_1(&msg.clone().into());
        handle.update(move |s, _| s.log.push(msg));
    }

    fn view(state: &State, _cx: &mut Cx<State>) -> El {
        let t = tok::theme(true);
        let mut lines = col().w_full().gap(4.0).py(12.0);
        for (i, l) in state.log.iter().enumerate() {
            lines = lines.child(text(l.clone()).key(format!("l{i}")).font(type_::BODY).color(t.text).w_full());
        }
        col().w_full().h_full().padding(16.0).fill(t.bg).gap(8.0)
            .child(text("thehideout web spike").font(type_::TITLE).color(t.text))
            .child(el().key("log").w_full().grow(1.0).shrink().scroll(ScrollSpec { stick_bottom: true }).child(lines))
    }

    fn param(search: &str, key: &str) -> Option<String> {
        search.trim_start_matches('?').split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            if k != key { return None; }
            js_sys::decode_uri_component(&v.replace("+", " ")).ok().map(String::from)
        })
    }

    async fn run(handle: Handle) -> Result<(), thehideout_api::ApiError> {
        let window = web_sys::window().expect("window");
        let location = window.location();
        let origin = location.origin().unwrap_or_default();
        let search = location.search().unwrap_or_default();
        let client = Client::new(origin.clone());
        line(&handle, format!("api: {origin} client header: {}", thehideout_api::client_header_value()));

        let user = param(&search, "user").unwrap_or_else(|| "spike".into());
        let pass = param(&search, "pass").unwrap_or_else(|| "spike-password-123".into());
        if let Some(code) = param(&search, "invite") {
            let preview = client.lookup_invite(&code).await?;
            line(&handle, format!("invite for {} (terms {})", preview.masked_email, preview.terms_version));
            let terms = thehideout_api::InviteTerms { version: &preview.terms_version, age_confirmed: true };
            client.accept_invite(&code, &user, Some(&pass), "web spike", terms).await?;
            line(&handle, format!("account {user} created"));
        } else {
            match client.login_on(&user, &pass, "web spike").await? {
                LoginOutcome::Session { user, .. } => line(&handle, format!("logged in as {} (id {})", user.username, user.id)),
                _ => line(&handle, "second factor required (not handled in the spike)"),
            }
        }

        // Unary call.
        let mut guilds = client.list_guilds().await?;
        line(&handle, format!("unary list_guilds -> {} guild(s): {:?}", guilds.len(), guilds.iter().map(|g| &g.name).collect::<Vec<_>>()));
        if guilds.is_empty() {
            guilds.push(client.create_guild("spike guild").await?);
            line(&handle, "unary create_guild -> spike guild");
        }
        let channels = client.list_channels(guilds[0].id).await?;
        line(&handle, format!("unary list_channels -> {:?}", channels.iter().map(|c| &c.name).collect::<Vec<_>>()));

        // Server-streaming: the gateway.
        let h = handle.clone();
        let gateway = Gateway::spawn(client.clone(), move |update| match update {
            GatewayUpdate::Event(Event::Heartbeat) => line(&h, "gateway event: Heartbeat"),
            GatewayUpdate::Event(e) => line(&h, format!("gateway event: {e:?}")),
            other => line(&h, format!("gateway: {other:?}")),
        });
        std::mem::forget(gateway); // keep it running for the life of the page

        // A second unary call whose effect must come back as a gateway event.
        if let Some(channel) = channels.first() {
            for i in 1..=2 {
                gloo_timers::future::sleep(std::time::Duration::from_millis(1500)).await;
                let m = client.send_message(channel.id, &format!("hello from the web spike #{i}"), Some(&format!("spike-{i}"))).await?;
                line(&handle, format!("unary send_message -> id {}", m.id));
            }
        }
        Ok(())
    }

    #[wasm_bindgen::prelude::wasm_bindgen(start)]
    pub fn start() {
        let app = App::new(AppSpec::new("web spike", (900.0, 700.0), State::default, view));
        let handle = app.handle();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = run(handle.clone()).await {
                line(&handle, format!("ERROR: {e}"));
            }
        });
        app.run();
    }
}
