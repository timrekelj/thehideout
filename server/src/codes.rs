//! Short secrets: emailed 6-digit codes, invite codes, recovery codes, and
//! TOTP (RFC 6238: SHA-1, 6 digits, 30 s steps, ±1 step of clock skew).

use rand::Rng;
use totp_rs::{Algorithm, Secret, TOTP};

/// Crockford base32 alphabet (no I, L, O, U).
const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A uniformly random 6-digit code.
pub fn six_digits() -> String {
    format!("{:06}", rand::rng().random_range(0..1_000_000u32))
}

/// Keeps only the digits of a typed code (`"123 456"` -> `"123456"`).
pub fn digits(raw: &str) -> String {
    raw.chars().filter(char::is_ascii_digit).collect()
}

fn crockford(len: usize) -> String {
    let mut rng = rand::rng();
    (0..len)
        .map(|_| CROCKFORD[rng.random_range(0..CROCKFORD.len())] as char)
        .collect()
}

/// An 8-character invite code (40 bits), as stored: no dash.
pub fn invite_code() -> String {
    crockford(8)
}

/// The stored form of a typed invite code (`abcd-efgh`, `ABCDEFGH`, ...), or `None` when it is
/// not exactly 8 Crockford characters.
pub fn parse_invite_code(raw: &str) -> Option<String> {
    let code = normalize_crockford(raw.trim());
    (code.len() == 8 && code.bytes().all(|b| CROCKFORD.contains(&b))).then_some(code)
}

/// `ABCDEFGH` -> `ABCD-EFGH`.
pub fn format_invite_code(code: &str) -> String {
    format!("{}-{}", &code[..4], &code[4..])
}

/// Canonical form of a typed Crockford code: uppercase, separators dropped,
/// I/L -> 1 and O -> 0.
pub fn normalize_crockford(raw: &str) -> String {
    raw.chars()
        .filter(|c| !matches!(c, '-' | ' ' | '_'))
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            c => c,
        })
        .collect()
}

/// A recovery code `xxxxx-xxxxx` (50 bits), lowercase.
pub fn recovery_code() -> String {
    let raw = crockford(10).to_lowercase();
    format!("{}-{}", &raw[..5], &raw[5..])
}

/// Canonical form of a typed recovery code (what gets hashed).
pub fn normalize_recovery(raw: &str) -> String {
    normalize_crockford(raw).to_lowercase()
}

pub const TOTP_STEP_SECS: u64 = 30;

/// A new random 160-bit TOTP secret, base32 (RFC 4648, no padding).
pub fn new_totp_secret() -> String {
    let mut bytes = [0u8; 20];
    rand::rng().fill(&mut bytes);
    Secret::Raw(bytes.to_vec()).to_encoded().to_string()
}

fn totp(secret: &str, account: &str) -> Option<TOTP> {
    let bytes = Secret::Encoded(secret.to_owned()).to_bytes().ok()?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        TOTP_STEP_SECS,
        bytes,
        Some("thehideout".to_owned()),
        account.to_owned(),
    )
    .ok()
}

/// `otpauth://totp/thehideout:<account>?secret=...&issuer=thehideout`.
pub fn otpauth_uri(secret: &str, account: &str) -> String {
    // `:` separates issuer and account in the label, so it cannot appear in the account.
    totp(secret, &account.replace(':', "_"))
        .map(|t| t.get_url())
        .unwrap_or_default()
}

/// The time step a code matches, within ±1 step of `now_secs`, and only if it
/// is newer than `last_step` (a code is accepted at most once).
pub fn verify_totp(secret: &str, code: &str, now_secs: u64, last_step: i64) -> Option<i64> {
    let code = digits(code);
    if code.len() != 6 {
        return None;
    }
    let t = totp(secret, "x")?;
    let current = (now_secs / TOTP_STEP_SECS) as i64;
    (current - 1..=current + 1)
        .filter(|step| *step > last_step && *step >= 0)
        .find(|step| {
            constant_eq(
                t.generate(*step as u64 * TOTP_STEP_SECS).as_bytes(),
                code.as_bytes(),
            )
        })
}

/// The current code (tests and the CLI).
pub fn totp_now(secret: &str, now_secs: u64) -> String {
    totp(secret, "x")
        .map(|t| t.generate(now_secs))
        .unwrap_or_default()
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_vector() {
        // RFC 6238 appendix B, SHA-1 secret "12345678901234567890", T=59 -> 94287082 (8 digits; 6 = 287082).
        let secret = Secret::Raw(b"12345678901234567890".to_vec())
            .to_encoded()
            .to_string();
        assert_eq!(totp_now(&secret, 59), "287082");
        assert_eq!(verify_totp(&secret, "287 082", 59, 0), Some(1));
    }

    #[test]
    fn totp_window_and_replay() {
        let secret = new_totp_secret();
        let now = 1_000_000_000;
        let step = (now / TOTP_STEP_SECS) as i64;
        let prev = totp_now(&secret, now - TOTP_STEP_SECS);
        let next = totp_now(&secret, now + TOTP_STEP_SECS);
        let far = totp_now(&secret, now + 3 * TOTP_STEP_SECS);
        assert_eq!(verify_totp(&secret, &prev, now, 0), Some(step - 1));
        assert_eq!(verify_totp(&secret, &next, now, 0), Some(step + 1));
        assert_eq!(verify_totp(&secret, &far, now, 0), None);
        // Replay: a step at or before the last accepted one is refused.
        assert_eq!(verify_totp(&secret, &prev, now, step - 1), None);
        assert!(otpauth_uri(&secret, "a@b.co")
            .starts_with("otpauth://totp/thehideout:a%40b.co?secret="));
    }

    #[test]
    fn codes_have_their_shapes() {
        assert_eq!(six_digits().len(), 6);
        assert_eq!(parse_invite_code("abcd-efgh").as_deref(), Some("ABCDEFGH"));
        assert_eq!(parse_invite_code(" ab cd_ef-gh ").as_deref(), Some("ABCDEFGH"));
        assert_eq!(parse_invite_code("oiLo-0110").as_deref(), Some("01100110"));
        for bad in ["", "ABCD-EFG", "ABCD-EFGHJ", "ABCD-EFGU", &"A".repeat(43)] {
            assert_eq!(parse_invite_code(bad), None, "{bad}");
        }
        let invite = invite_code();
        assert_eq!(invite.len(), 8);
        assert_eq!(
            normalize_crockford(&format_invite_code(&invite).to_lowercase()),
            invite
        );
        assert_eq!(normalize_crockford("o1l-i"), "0111");
        let rc = recovery_code();
        assert_eq!(rc.len(), 11);
        assert_eq!(
            normalize_recovery(&rc.to_uppercase()),
            normalize_recovery(&rc)
        );
    }
}
