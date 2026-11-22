//! Text + HTML bodies of every email the server sends.

use html_escape::encode_text;

use super::Email;

/// Wraps paragraphs into a minimal, client-safe HTML document.
fn html(title: &str, paragraphs: &[String]) -> String {
    let body: String = paragraphs
        .iter()
        .map(|p| format!("<p style=\"margin:0 0 16px\">{p}</p>"))
        .collect();
    format!(
        "<!doctype html><html><body style=\"font-family:-apple-system,Segoe UI,sans-serif;color:#1b1b1f;max-width:520px;margin:24px auto;padding:0 16px\">\
         <h2 style=\"font-size:18px;margin:0 0 16px\">{}</h2>{body}\
         <p style=\"color:#6b6b76;font-size:12px;margin-top:32px\">thehideout</p></body></html>",
        encode_text(title)
    )
}

fn code_html(code: &str) -> String {
    format!(
        "<span style=\"font-family:ui-monospace,Menlo,monospace;font-size:24px;letter-spacing:4px;font-weight:600\">{}</span>",
        encode_text(code)
    )
}

fn email(kind: &'static str, to: &str, key: String, subject: &str, text: String, html: String) -> Email {
    Email {
        kind,
        to: to.to_owned(),
        subject: subject.to_owned(),
        text,
        html,
        idempotency_key: key,
    }
}

/// "a", "a and b", "a, b and c".
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Invitation with the link and the short code.
pub fn invite(
    to: &str,
    key: String,
    inviter: Option<&str>,
    guilds: &[String],
    link: &str,
    code: &str,
) -> Email {
    // Invites from the CLI or the bootstrap have no inviter.
    let invited = inviter.map_or_else(
        || "You've been invited".to_owned(),
        |name| format!("{name} has invited you"),
    );
    let into = if guilds.is_empty() {
        String::new()
    } else {
        format!(" and to join {}", list(guilds))
    };
    let lead = format!("{invited} to create an account on thehideout{into}.");
    let text = format!(
        "{lead}\n\n\
         Accept the invite: {link}\n\nOr enter this code in the app: {code}\n\n\
         The invite expires in 7 days. If you weren't expecting it, ignore this email."
    );
    let html = html(
        "You're invited to thehideout",
        &[
            encode_text(&lead).into_owned(),
            format!(
                "<a href=\"{0}\">Accept the invite</a> &middot; {0}",
                encode_text(link)
            ),
            format!("Or enter this code in the app: {}", code_html(code)),
            "The invite expires in 7 days. If you weren't expecting it, ignore this email.".into(),
        ],
    );
    email("invite", to, key, "You're invited to thehideout", text, html)
}

/// Sign-in code.
pub fn sign_in_code(to: &str, key: String, code: &str) -> Email {
    let text = format!(
        "Your thehideout sign-in code is {code}\n\nIt expires in 10 minutes. If you didn't try to sign in, ignore this email."
    );
    let html = html(
        "Your sign-in code",
        &[
            code_html(code),
            "It expires in 10 minutes. If you didn't try to sign in, ignore this email.".into(),
        ],
    );
    email(
        "sign_in_code",
        to,
        key,
        &format!("{code} is your thehideout sign-in code"),
        text,
        html,
    )
}

/// Password reset code.
pub fn password_reset(to: &str, key: String, code: &str) -> Email {
    let text = format!(
        "Your thehideout password reset code is {code}\n\nIt expires in 10 minutes. If you didn't ask to reset your password, ignore this email."
    );
    let html = html(
        "Reset your password",
        &[
            code_html(code),
            "It expires in 10 minutes. If you didn't ask to reset your password, ignore this email.".into(),
        ],
    );
    email("password_reset", to, key, "Reset your thehideout password", text, html)
}

/// Code confirming a new address.
pub fn email_change_code(to: &str, key: String, code: &str) -> Email {
    let text = format!(
        "Enter {code} in thehideout to use this address for your account.\n\nIt expires in 10 minutes. If you didn't ask for this, ignore this email."
    );
    let html = html(
        "Confirm your email",
        &[
            format!(
                "Enter {} in thehideout to use this address for your account.",
                code_html(code)
            ),
            "It expires in 10 minutes. If you didn't ask for this, ignore this email.".into(),
        ],
    );
    email("email_change_code", to, key, "Confirm your thehideout email", text, html)
}

/// Sent to the old address after an email change.
pub fn email_changed(to: &str, key: String, new_email: &str) -> Email {
    let masked = crate::accounts::mask_email(new_email);
    let text = format!(
        "The email of your thehideout account was changed to {masked}.\n\nIf this wasn't you, reset your password and contact an admin."
    );
    let html = html(
        "Your email was changed",
        &[
            format!(
                "The email of your thehideout account was changed to {}.",
                encode_text(&masked)
            ),
            "If this wasn't you, reset your password and contact an admin.".into(),
        ],
    );
    email("email_changed", to, key, "Your thehideout email was changed", text, html)
}

/// A sign-in from a device the account has not used before.
pub fn new_device(to: &str, key: String, device: &str, method: &str) -> Email {
    let text = format!(
        "Your thehideout account was signed in on a new device: {device} (with {method}).\n\n\
         If this wasn't you, sign out that session in Settings, change your password and turn on two-factor authentication."
    );
    let html = html(
        "New sign-in",
        &[
            format!(
                "Your thehideout account was signed in on a new device: <b>{}</b> (with {}).",
                encode_text(device),
                encode_text(method)
            ),
            "If this wasn't you, sign out that session in Settings, change your password and turn on two-factor authentication."
                .into(),
        ],
    );
    email("new_device", to, key, "New sign-in to thehideout", text, html)
}

/// A security setting changed (password, two-factor, passkeys).
pub fn security_change(to: &str, key: String, what: &str) -> Email {
    let text = format!("Security change on your thehideout account: {what}.\n\nIf this wasn't you, reset your password and contact an admin.");
    let html = html(
        "Security change",
        &[
            format!(
                "Security change on your thehideout account: <b>{}</b>.",
                encode_text(what)
            ),
            "If this wasn't you, reset your password and contact an admin.".into(),
        ],
    );
    email(
        "security_change",
        to,
        key,
        "Security change on your thehideout account",
        text,
        html,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_has_link_and_code_in_both_parts_and_escapes() {
        let e = invite(
            "a@x.org",
            "k".into(),
            Some("<b>"),
            &["g".into()],
            "https://h.example/invite/T",
            "ABCD-EFGH",
        );
        for part in [&e.text, &e.html] {
            assert!(part.contains("https://h.example/invite/T"));
            assert!(part.contains("ABCD-EFGH"));
        }
        assert!(e.html.contains("&lt;b&gt;") && !e.html.contains("<b> has"));
    }

    fn invite_lead(inviter: Option<&str>, guilds: &[&str]) -> Email {
        let guilds: Vec<String> = guilds.iter().map(|g| (*g).to_owned()).collect();
        invite(
            "a@x.org",
            "k".into(),
            inviter,
            &guilds,
            "https://h.example/invite/T",
            "ABCD-EFGH",
        )
    }

    /// The first sentence must appear verbatim in the text and HTML parts.
    fn assert_lead(e: &Email, lead: &str) {
        assert!(e.text.starts_with(lead), "text: {}", e.text);
        assert!(e.html.contains(&format!(">{lead}</p>")), "html: {}", e.html);
        assert!(!e.text.contains("You have invited you"));
    }

    #[test]
    fn invite_without_inviter_or_guilds() {
        assert_lead(
            &invite_lead(None, &[]),
            "You've been invited to create an account on thehideout.",
        );
    }

    #[test]
    fn invite_without_inviter_with_guilds() {
        assert_lead(
            &invite_lead(None, &["Den", "Lab", "Pub"]),
            "You've been invited to create an account on thehideout and to join Den, Lab and Pub.",
        );
    }

    #[test]
    fn invite_with_inviter_without_guilds() {
        assert_lead(
            &invite_lead(Some("tim"), &[]),
            "tim has invited you to create an account on thehideout.",
        );
    }

    #[test]
    fn invite_with_inviter_and_guilds() {
        assert_lead(
            &invite_lead(Some("tim"), &["Den"]),
            "tim has invited you to create an account on thehideout and to join Den.",
        );
        assert_lead(
            &invite_lead(Some("tim"), &["Den", "Lab"]),
            "tim has invited you to create an account on thehideout and to join Den and Lab.",
        );
    }

    #[test]
    fn code_emails_carry_the_code_and_address_the_reader() {
        for e in [
            sign_in_code("a@x.org", "k".into(), "123456"),
            password_reset("a@x.org", "k".into(), "123456"),
            email_change_code("a@x.org", "k".into(), "123456"),
        ] {
            assert!(
                e.text.contains("123456") && e.html.contains("123456"),
                "{e:?}"
            );
            assert!(e.text.contains("If you didn't"), "{e:?}");
        }
        assert_eq!(
            sign_in_code("a@x.org", "k".into(), "123456").subject,
            "123456 is your thehideout sign-in code"
        );
        assert!(password_reset("a@x.org", "k".into(), "1")
            .text
            .starts_with("Your thehideout password reset code is 1\n"));
        assert!(email_change_code("a@x.org", "k".into(), "1")
            .text
            .starts_with("Enter 1 in thehideout to use this address"));
    }

    #[test]
    fn notice_emails_read_as_sentences() {
        let e = email_changed("a@x.org", "k".into(), "new@x.org");
        assert!(
            e.text
                .starts_with("The email of your thehideout account was changed to ")
                && !e.text.contains("new@x.org")
        );
        let e = new_device("a@x.org", "k".into(), "Laptop", "a password");
        assert!(e.text.starts_with(
            "Your thehideout account was signed in on a new device: Laptop (with a password).\n"
        ));
        assert!(e.html.contains("<b>Laptop</b> (with a password)."));
        let e = security_change(
            "a@x.org",
            "k".into(),
            "two-factor authentication was turned on",
        );
        assert!(e.text.starts_with("Security change on your thehideout account: two-factor authentication was turned on.\n"));
    }

    #[test]
    fn list_joins_with_and() {
        assert_eq!(list(&[]), "");
        assert_eq!(list(&["a".into()]), "a");
        assert_eq!(list(&["a".into(), "b".into()]), "a and b");
        assert_eq!(list(&["a".into(), "b".into(), "c".into()]), "a, b and c");
    }
}
