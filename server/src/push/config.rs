//! Push provider settings from the environment.
//!
//! APNs: `THEHIDEOUT_APNS_KEY_PATH` (a `.p8` file) or `THEHIDEOUT_APNS_KEY` (its
//! PEM text), `THEHIDEOUT_APNS_KEY_ID`, `THEHIDEOUT_APNS_TEAM_ID`,
//! `THEHIDEOUT_APNS_TOPIC` (the bundle id, default `si.zabit.thehideout`) and
//! `THEHIDEOUT_APNS_SANDBOX`. FCM: `THEHIDEOUT_FCM_SERVICE_ACCOUNT` (a path to
//! the JSON key file, or the JSON itself). Each provider is off unless set.

use anyhow::{Context, Result, bail};

use crate::config::Redacted;

pub const DEFAULT_APNS_TOPIC: &str = "si.zabit.thehideout";

#[derive(Debug, Clone, Default)]
pub struct PushConfig {
    pub apns: Option<ApnsConfig>,
    pub fcm: Option<FcmConfig>,
}

#[derive(Debug, Clone)]
pub struct ApnsConfig {
    /// PEM text of the `.p8` key.
    pub key_pem: Redacted,
    pub key_id: String,
    pub team_id: String,
    /// Bundle id.
    pub topic: String,
    /// Send every iOS push through `api.sandbox.push.apple.com`.
    pub sandbox: bool,
}

#[derive(Debug, Clone)]
pub struct FcmConfig {
    /// The service-account key (JSON text).
    pub service_account_json: Redacted,
}

impl PushConfig {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|name| std::env::var(name).ok(), |path| std::fs::read_to_string(path))
    }

    /// [`from_env`](Self::from_env) over any variable lookup and file reader (tests).
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
        read: impl Fn(&str) -> std::io::Result<String>,
    ) -> Result<Self> {
        let get = |name: &str| lookup(name).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        let apns = {
            let path = get("THEHIDEOUT_APNS_KEY_PATH");
            let inline = get("THEHIDEOUT_APNS_KEY");
            let key_id = get("THEHIDEOUT_APNS_KEY_ID");
            let team_id = get("THEHIDEOUT_APNS_TEAM_ID");
            if path.is_none() && inline.is_none() && key_id.is_none() && team_id.is_none() {
                None
            } else {
                let (Some(key_id), Some(team_id)) = (key_id, team_id) else {
                    bail!("THEHIDEOUT_APNS_KEY_ID and THEHIDEOUT_APNS_TEAM_ID are required with an APNs key (or unset all THEHIDEOUT_APNS_*)");
                };
                let key_pem = match (inline, path) {
                    (Some(pem), _) => pem.replace("\\n", "\n"),
                    (None, Some(path)) => read(&path).with_context(|| format!("reading THEHIDEOUT_APNS_KEY_PATH {path:?}"))?,
                    (None, None) => bail!("THEHIDEOUT_APNS_KEY_PATH or THEHIDEOUT_APNS_KEY is required (or unset all THEHIDEOUT_APNS_*)"),
                };
                super::jwt::Es256Key::from_pem(&key_pem).context("THEHIDEOUT_APNS_KEY is not a usable .p8 key")?;
                let sandbox = match get("THEHIDEOUT_APNS_SANDBOX").unwrap_or_default().to_lowercase().as_str() {
                    "true" | "1" | "yes" => true,
                    "" | "false" | "0" | "no" => false,
                    other => bail!("invalid THEHIDEOUT_APNS_SANDBOX {other:?}, expected true or false"),
                };
                Some(ApnsConfig {
                    key_pem: Redacted(key_pem),
                    key_id,
                    team_id,
                    topic: get("THEHIDEOUT_APNS_TOPIC").unwrap_or_else(|| DEFAULT_APNS_TOPIC.to_owned()),
                    sandbox,
                })
            }
        };
        let fcm = match get("THEHIDEOUT_FCM_SERVICE_ACCOUNT") {
            None => None,
            Some(value) => {
                let json = if value.starts_with('{') {
                    value
                } else {
                    read(&value).with_context(|| format!("reading THEHIDEOUT_FCM_SERVICE_ACCOUNT {value:?}"))?
                };
                super::fcm::ServiceAccount::parse(&json).context("THEHIDEOUT_FCM_SERVICE_ACCOUNT is not a usable service-account key")?;
                Some(FcmConfig { service_account_json: Redacted(json) })
            }
        };
        Ok(Self { apns, fcm })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::testkeys::{EC_P8, RSA_PEM};

    fn from(vars: &[(&str, &str)]) -> Result<PushConfig> {
        PushConfig::from_lookup(
            |n| vars.iter().find(|(k, _)| *k == n).map(|(_, v)| v.to_string()),
            |p| if p == "/key.p8" { Ok(EC_P8.to_owned()) } else { Err(std::io::ErrorKind::NotFound.into()) },
        )
    }

    #[test]
    fn unset_means_disabled() {
        let c = from(&[]).unwrap();
        assert!(c.apns.is_none() && c.fcm.is_none());
        assert!(from(&[("THEHIDEOUT_APNS_KEY_ID", " ")]).unwrap().apns.is_none());
    }

    #[test]
    fn apns_needs_everything_and_a_real_key() {
        let ok = from(&[("THEHIDEOUT_APNS_KEY_PATH", "/key.p8"), ("THEHIDEOUT_APNS_KEY_ID", "K"), ("THEHIDEOUT_APNS_TEAM_ID", "T")]).unwrap().apns.unwrap();
        assert_eq!((ok.topic.as_str(), ok.sandbox), ("si.zabit.thehideout", false));
        assert!(from(&[("THEHIDEOUT_APNS_KEY_PATH", "/key.p8")]).is_err());
        assert!(from(&[("THEHIDEOUT_APNS_KEY_ID", "K"), ("THEHIDEOUT_APNS_TEAM_ID", "T")]).is_err());
        assert!(from(&[("THEHIDEOUT_APNS_KEY_PATH", "/nope"), ("THEHIDEOUT_APNS_KEY_ID", "K"), ("THEHIDEOUT_APNS_TEAM_ID", "T")]).is_err());
        assert!(from(&[("THEHIDEOUT_APNS_KEY", "junk"), ("THEHIDEOUT_APNS_KEY_ID", "K"), ("THEHIDEOUT_APNS_TEAM_ID", "T")]).is_err());
        let inline = EC_P8.replace('\n', "\\n");
        let c = from(&[("THEHIDEOUT_APNS_KEY", &inline), ("THEHIDEOUT_APNS_KEY_ID", "K"), ("THEHIDEOUT_APNS_TEAM_ID", "T"), ("THEHIDEOUT_APNS_SANDBOX", "true"), ("THEHIDEOUT_APNS_TOPIC", "x.y")]).unwrap().apns.unwrap();
        assert_eq!((c.topic.as_str(), c.sandbox), ("x.y", true));
        assert!(from(&[("THEHIDEOUT_APNS_KEY", &inline), ("THEHIDEOUT_APNS_KEY_ID", "K"), ("THEHIDEOUT_APNS_TEAM_ID", "T"), ("THEHIDEOUT_APNS_SANDBOX", "maybe")]).is_err());
    }

    #[test]
    fn fcm_takes_json_or_a_path_and_never_prints_the_key() {
        let json = serde_json::json!({ "client_email": "a@b.iam.gserviceaccount.com", "private_key": RSA_PEM, "project_id": "p" }).to_string();
        let c = from(&[("THEHIDEOUT_FCM_SERVICE_ACCOUNT", &json)]).unwrap();
        assert!(c.fcm.is_some());
        assert!(!format!("{c:?}").contains("BEGIN"));
        assert!(from(&[("THEHIDEOUT_FCM_SERVICE_ACCOUNT", "/missing.json")]).is_err());
        assert!(from(&[("THEHIDEOUT_FCM_SERVICE_ACCOUNT", "{}")]).is_err());
    }
}
