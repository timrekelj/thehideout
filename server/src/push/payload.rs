//! What a push says: the plain-text preview, @mention parsing (the same
//! syntax the clients render, `client/design/.../markdown.rs`) and the
//! provider-neutral [`Notification`].

use std::collections::BTreeSet;

/// Longest preview body, in characters (including the ellipsis).
pub const MAX_BODY_CHARS: usize = 180;
/// Body sent when the recipient turned previews off.
pub const GENERIC_BODY: &str = "New message";

/// Why a push is sent; sent to the app as `kind` for deep linking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A message in a guild channel.
    Message,
    /// A message in a DM channel.
    Dm,
    /// A guild channel message that @mentions the recipient.
    Mention,
    /// The recipient was added to a guild.
    GuildAdded,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Message => "message",
            Kind::Dm => "dm",
            Kind::Mention => "mention",
            Kind::GuildAdded => "guild_added",
        }
    }

    /// Android notification channel id.
    pub fn android_channel(self) -> &'static str {
        match self {
            Kind::Mention | Kind::Dm => "mentions",
            Kind::Message | Kind::GuildAdded => "messages",
        }
    }
}

/// A notification to deliver, independent of APNs / FCM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub kind: Kind,
    pub title: String,
    pub body: String,
    /// 0 when not applicable (DMs, other kinds).
    pub guild_id: u64,
    /// 0 for `guild_added`.
    pub channel_id: u64,
    pub message_id: u64,
}

impl Notification {
    /// Thread / collapse id: the channel (the guild for `guild_added`).
    pub fn collapse_id(&self) -> String {
        if self.channel_id != 0 { self.channel_id } else { self.guild_id }.to_string()
    }

    /// The deep-link data the apps receive (all values are strings).
    pub fn data(&self) -> [(&'static str, String); 4] {
        [
            ("kind", self.kind.as_str().to_owned()),
            ("guild_id", self.guild_id.to_string()),
            ("channel_id", self.channel_id.to_string()),
            ("message_id", self.message_id.to_string()),
        ]
    }
}

/// `@usernames` in `content`, lowercased and deduplicated. Mirrors the client
/// parser: an `@` that does not follow a letter or digit, then letters, digits,
/// `_`, `.` and `-`, minus trailing `.`, `-`, `_`. Code spans and fences are
/// skipped (the client renders no mentions inside them).
pub fn mentioned_usernames(content: &str) -> BTreeSet<String> {
    let chars: Vec<char> = without_code(content).chars().collect();
    let is_mention_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '-');
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < chars.len() {
        let word_start = i == 0 || !chars[i - 1].is_alphanumeric();
        if chars[i] == '@' && word_start {
            let run = chars[i + 1..].iter().take_while(|&&c| is_mention_char(c)).count();
            let name = chars[i + 1..i + 1 + run].iter().rposition(|&c| !matches!(c, '.' | '-' | '_')).map_or(0, |p| p + 1);
            if name > 0 {
                out.insert(chars[i + 1..i + 1 + name].iter().collect::<String>().to_lowercase());
                i += name + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `content` with inline code spans and fenced blocks removed (a backtick run
/// closes at a run of the same length; an unclosed run is literal).
fn without_code(content: &str) -> String {
    let chars: Vec<char> = content.chars().collect();
    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            let run = chars[i..].iter().take_while(|&&c| c == '`').count();
            let mut p = i + run;
            let mut close = None;
            while p < chars.len() {
                if chars[p] == '`' {
                    let r = chars[p..].iter().take_while(|&&c| c == '`').count();
                    if r == run {
                        close = Some(p + r);
                        break;
                    }
                    p += r;
                } else {
                    p += 1;
                }
            }
            match close {
                Some(end) => {
                    out.push(' ');
                    i = end;
                }
                None => {
                    out.extend(std::iter::repeat_n('`', run));
                    i += run;
                }
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// A plain-text preview of message markdown: emphasis, spoiler, strike and code
/// markers dropped, custom emoji as `:name:`, whitespace collapsed, cut to
/// [`MAX_BODY_CHARS`] with an ellipsis.
pub fn preview(content: &str) -> String {
    let text = strip_markdown(content);
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&collapsed, MAX_BODY_CHARS)
}

fn strip_markdown(content: &str) -> String {
    let chars: Vec<char> = content.chars().collect();
    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let prev = i.checked_sub(1).map(|p| chars[p]);
        match c {
            '\\' if next.is_some_and(|n| n.is_ascii_punctuation()) => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '`' | '*' => i += 1,
            '~' | '|' => {
                // Only doubled markers (`~~`, `||`) are markup.
                if next != Some(c) && prev != Some(c) {
                    out.push(c);
                }
                i += 1;
            }
            '_' => {
                // Emphasis markers sit at a word edge; `snake_case` keeps its underscores.
                let opens = prev.is_none_or(|p| !p.is_alphanumeric()) && next.is_some_and(|n| n.is_alphanumeric() || n == '_' || n == '@');
                let closes = next.is_none_or(|n| !n.is_alphanumeric()) && prev.is_some_and(|p| p.is_alphanumeric() || p == '_' || p == '.');
                if opens || closes { i += 1 } else {
                    out.push('_');
                    i += 1;
                }
            }
            '<' => {
                // Custom emoji `<:name:id>` / `<a:name:id>`.
                let rest: String = chars[i..].iter().take(80).collect();
                if let Some(end) = rest.find('>') {
                    let inner = &rest[1..end];
                    let inner = inner.strip_prefix('a').filter(|s| s.starts_with(':')).unwrap_or(inner);
                    let mut parts = inner.split(':');
                    if let (Some(""), Some(name), Some(id), None) = (parts.next(), parts.next(), parts.next(), parts.next())
                        && !name.is_empty()
                        && id.bytes().all(|b| b.is_ascii_digit())
                        && !id.is_empty()
                    {
                        out.push(':');
                        out.push_str(name);
                        out.push(':');
                        i += rest[..=end].chars().count();
                        continue;
                    }
                }
                out.push('<');
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// At most `max` characters; longer text is cut and ends with `…`.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max - 1).collect();
    while cut.ends_with(' ') {
        cut.pop();
    }
    cut.push('…');
    cut
}

/// The body of a message push: the preview (or "Sent an attachment" for a
/// message without text), or the generic text when previews are off.
pub fn body(content: &str, has_attachments: bool, show_previews: bool) -> String {
    if !show_previews {
        return GENERIC_BODY.to_owned();
    }
    let text = preview(content);
    if text.is_empty() {
        if has_attachments { "Sent an attachment".to_owned() } else { GENERIC_BODY.to_owned() }
    } else {
        text
    }
}

/// `Ana` in a DM, `Ana in #general · Hideout` in a guild.
pub fn title(sender: &str, place: Option<(&str, &str)>) -> String {
    match place {
        Some((channel, guild)) => format!("{sender} in #{channel} · {guild}"),
        None => sender.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(s: &str) -> Vec<String> {
        mentioned_usernames(s).into_iter().collect()
    }

    #[test]
    fn mentions_follow_the_client_syntax() {
        assert_eq!(names("ping @ana.k, thanks @Bruno."), ["ana.k", "bruno"]);
        assert!(names("mail a@b.co").is_empty());
        assert_eq!(names("**hi @ana**"), ["ana"]);
        assert_eq!(names("__@ana__ and ||@tim||"), ["ana", "tim"]);
        assert_eq!(names("@a-b_c @ana @ana"), ["a-b_c", "ana"]);
        assert!(names("lone @ or @. or @-").is_empty());
        assert_eq!(names("(@ana)"), ["ana"]);
    }

    #[test]
    fn mentions_in_code_do_not_count() {
        assert!(names("run `@ana` now").is_empty());
        assert!(names("```\n@ana\n```").is_empty());
        assert_eq!(names("`x` then @ana"), ["ana"]);
        assert_eq!(names("`unclosed @ana"), ["ana"]);
    }

    #[test]
    fn markdown_is_stripped_from_previews() {
        assert_eq!(preview("**bold** and *it* and ~~gone~~ and ||spoiler||"), "bold and it and gone and spoiler");
        assert_eq!(preview("__under__ _it_ snake_case"), "under it snake_case");
        assert_eq!(preview("run `cargo test` now"), "run cargo test now");
        assert_eq!(preview("hi <:blob:123> and <a:party:9>!"), "hi :blob: and :party:!");
        assert_eq!(preview("line one\n\n  line   two"), "line one line two");
        assert_eq!(preview("1 < 2 and a | b and 3 * 4"), "1 < 2 and a | b and 3 4");
        assert_eq!(preview("escaped \\*star\\*"), "escaped *star*");
        assert_eq!(preview("hey @ana, look"), "hey @ana, look");
    }

    #[test]
    fn previews_are_cut_at_180_characters() {
        let long = "é".repeat(400);
        let p = preview(&long);
        assert_eq!(p.chars().count(), MAX_BODY_CHARS);
        assert!(p.ends_with('…'));
        let exact = "x".repeat(MAX_BODY_CHARS);
        assert_eq!(preview(&exact), exact);
    }

    #[test]
    fn bodies_and_titles() {
        assert_eq!(body("secret", false, false), "New message");
        assert_eq!(body("hello **you**", false, true), "hello you");
        assert_eq!(body("", true, true), "Sent an attachment");
        assert_eq!(title("Ana", None), "Ana");
        assert_eq!(title("Ana", Some(("general", "Hideout"))), "Ana in #general · Hideout");
    }

    #[test]
    fn data_and_collapse_id() {
        let push = Notification { kind: Kind::Mention, title: "t".into(), body: "b".into(), guild_id: 1, channel_id: 2, message_id: 3 };
        assert_eq!(push.collapse_id(), "2");
        assert_eq!(push.data()[0], ("kind", "mention".to_owned()));
        let joined = Notification { kind: Kind::GuildAdded, channel_id: 0, ..push };
        assert_eq!(joined.collapse_id(), "1");
    }
}
