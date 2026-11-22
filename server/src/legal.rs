//! `GET /privacy` and `GET /terms`: the privacy notice and terms of use, in
//! plain language for a small invite-only community. Server-rendered with the
//! same restrictive headers as the invite landing page.

use axum::Router;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use html_escape::encode_text;

use crate::AppState;
use crate::policy::{CONTACT_PLACEHOLDER, LEGAL_VERSION, MIN_AGE, OPERATOR_PLACEHOLDER};

/// How long server logs are kept.
pub const LOG_RETENTION_DAYS: u32 = 30;
/// How long backups are kept.
pub const BACKUP_RETENTION_DAYS: u32 = 30;
/// How long a data export download stays available.
pub const EXPORT_RETENTION_DAYS: u32 = 7;

pub fn routes() -> Router<AppState> {
    Router::new().route("/privacy", get(privacy)).route("/terms", get(terms))
}

fn page(title: &str, body: &str) -> impl IntoResponse + use<> {
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\">\
         <title>{title} - thehideout</title></head>\
         <body style=\"font-family:-apple-system,Segoe UI,sans-serif;max-width:680px;margin:6vh auto;padding:0 16px;color:#1b1b1f;line-height:1.55\">\
         <p style=\"font-size:13px;color:#6b6b76\">Version {LEGAL_VERSION} &middot; <a href=\"/privacy\">Privacy</a> &middot; <a href=\"/terms\">Terms</a></p>\
         <h1 style=\"font-size:26px\">{title}</h1>{body}</body></html>"
    );
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
            // No scripts or remote content; inline style attributes only.
            (header::HeaderName::from_static("content-security-policy"), "default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'"),
            (header::HeaderName::from_static("x-frame-options"), "DENY"),
            (header::HeaderName::from_static("x-content-type-options"), "nosniff"),
            (header::HeaderName::from_static("referrer-policy"), "no-referrer"),
        ],
        html,
    )
}

fn h2(text: &str) -> String {
    format!("<h2 style=\"font-size:18px;margin-top:28px\">{text}</h2>")
}

fn operator(state: &AppState) -> String {
    encode_text(state.policy.operator_name.as_deref().unwrap_or(OPERATOR_PLACEHOLDER)).into_owned()
}

fn contact(state: &AppState) -> String {
    let email = encode_text(state.policy.contact_email.as_deref().unwrap_or(CONTACT_PLACEHOLDER));
    match &state.policy.contact_email {
        Some(_) => format!("<a href=\"mailto:{email}\">{email}</a>"),
        None => format!("<b>{email}</b>"),
    }
}

/// The privacy notice as the body of a page.
pub fn privacy_body(state: &AppState) -> String {
    let (operator, contact) = (operator(state), contact(state));
    format!(
        "<p>thehideout is a small, invite-only chat service run by {operator} (&ldquo;we&rdquo;). \
         This page says what we keep about you, why, and what you can do about it. Questions: {contact}.</p>\
         {w}<ul>\
         <li><b>Your account:</b> username, email address, a hash of your password (never the password), \
         security settings (authenticator app secret, passkeys, recovery codes), your avatar, and when you joined.</li>\
         <li><b>Your devices:</b> a list of your signed-in sessions (device name, when created, roughly when last used).</li>\
         <li><b>Your content:</b> messages, edits, reactions, direct messages, files you upload and custom emojis you add, \
         and which servers and roles you have.</li>\
         <li><b>Voice and video:</b> calls are relayed live and are <b>not recorded or stored</b>.</li>\
         <li><b>Technical data:</b> your IP address appears in server logs and in short-lived rate-limit counters \
         that protect sign-in from abuse.</li>\
         <li><b>Moderation records:</b> actions by admins and server moderators (bans, kicks, deleted files) are logged with who did them.</li>\
         </ul>\
         {why}<p>To run the service: sign you in, deliver your messages to the people you chat with, keep the service secure, \
         and email you things you asked for or need to know (invites, sign-in codes, security notices). \
         We do not sell data, show ads or profile you.</p>\
         {keep}<ul>\
         <li>Messages and files stay until you or a moderator delete them, or until the server they are in is deleted.</li>\
         <li>Server logs: {LOG_RETENTION_DAYS} days. Backups: {BACKUP_RETENTION_DAYS} days, so deleted data can still sit in a backup for up to that long.</li>\
         <li>Data exports you request: {EXPORT_RETENTION_DAYS} days, then the download is deleted.</li>\
         <li>If you delete your account, your username, email, avatar, password, security settings, sessions and uploads are removed. \
         Your messages stay in the conversations you had, shown as &ldquo;Deleted user&rdquo;, without your name attached.</li>\
         </ul>\
         {share}<p>We do not share your data with anyone for their own purposes. Two service providers handle data for us: \
         an email provider (Resend) that delivers our emails, and our hosting provider. They only process data to provide those services.</p>\
         {rights}<ul>\
         <li><b>Export:</b> in the app, choose &ldquo;Export my data&rdquo;; we email you a link to a file with your data (once per 24 hours).</li>\
         <li><b>Delete:</b> in the app, choose &ldquo;Delete account&rdquo;. If you own servers, transfer or delete them first.</li>\
         <li><b>Correct or ask:</b> change your username, email and avatar in settings, or write to {contact} \
         to ask about, correct or object to how your data is used. If you are in the EU you may also complain to your data protection authority.</li>\
         </ul>\
         {cookies}<p>The website parts of this service (this page, invite pages) set no cookies and run no scripts or trackers. \
         The apps keep a sign-in token on your device.</p>\
         {age}<p>You must be at least {MIN_AGE} years old to use thehideout. We ask you to confirm this when you create an account.</p>\
         {sec}<p>Connections are encrypted, passwords are stored hashed, and you can turn on an authenticator app or passkeys. \
         Messages are not end-to-end encrypted: the server and its operators can technically read them, and we only look when needed to \
         run or moderate the service or when the law requires it.</p>\
         {changes}<p>If we change this notice in a way that matters, the version date at the top changes and the app asks you to accept it again.</p>",
        w = h2("What we store"),
        why = h2("Why"),
        keep = h2("How long we keep it"),
        share = h2("Who gets it"),
        rights = h2("Your rights"),
        cookies = h2("Cookies"),
        age = h2("Minimum age"),
        sec = h2("Security"),
        changes = h2("Changes"),
    )
}

/// The terms of use as the body of a page.
pub fn terms_body(state: &AppState) -> String {
    let (operator, contact) = (operator(state), contact(state));
    format!(
        "<p>These are the rules for using thehideout, run by {operator}. By creating an account you agree to them and to the \
         <a href=\"/privacy\">privacy notice</a>.</p>\
         {a}<ul><li>You are at least {MIN_AGE} years old.</li>\
         <li>Your account is yours alone. Keep your password and devices safe and tell us if something looks wrong.</li></ul>\
         {b}<ul><li>You are responsible for what you post. Be decent to the people here.</li>\
         <li>Do not post anything illegal, harass or threaten people, share other people&rsquo;s private data, \
         spam, or upload malware.</li>\
         <li>Do not try to break, overload or probe the service, or get around its limits and permissions.</li>\
         <li>Respect copyright: only upload what you may share.</li></ul>\
         {c}<p>Server owners and moderators can remove content and members from their servers; site admins can remove content, \
         suspend accounts that break these rules and delete accounts. When we can, we tell you why.</p>\
         {d}<p>You keep the rights to what you post. You allow us to store it and show it to the people you send it to, \
         because that is what the service does.</p>\
         {e}<p>This is a small, invite-only service run on a best-effort basis. It is provided &ldquo;as is&rdquo;, \
         it can be down or change, and we may stop it with notice where possible. To the extent the law allows, \
         we are not liable for losses from using it. Nothing here limits rights you have by law.</p>\
         {f}<p>You can delete your account in the app at any time. We may suspend or delete accounts that break these terms.</p>\
         {g}<p>If we change these terms in a way that matters, the version date at the top changes and the app asks you to accept the new version.</p>\
         {h}<p>{contact}</p>",
        a = h2("Who can use it"),
        b = h2("What you may and may not do"),
        c = h2("Moderation"),
        d = h2("Your content"),
        e = h2("The service"),
        f = h2("Ending your account"),
        g = h2("Changes"),
        h = h2("Contact"),
    )
}

async fn privacy(State(state): State<AppState>) -> impl IntoResponse {
    let body = privacy_body(&state);
    page("Privacy notice", &body)
}

async fn terms(State(state): State<AppState>) -> impl IntoResponse {
    let body = terms_body(&state);
    page("Terms of use", &body)
}
