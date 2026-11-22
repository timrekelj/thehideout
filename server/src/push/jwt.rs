//! Signed JWTs for the push providers: ES256 for APNs (token-based auth) and
//! RS256 for Google's OAuth2 service-account flow. Signing uses `ring`
//! (already a dependency of rustls), so there is no JWT crate.

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, RSA_PKCS1_SHA256, RsaKeyPair};

/// The DER bytes of a PEM block (`-----BEGIN ...-----` ... `-----END ...-----`).
pub fn pem_to_der(pem: &str) -> Result<Vec<u8>> {
    let body: String = pem.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("-----")).collect();
    if body.is_empty() {
        bail!("empty PEM");
    }
    STANDARD.decode(body).context("PEM is not valid base64")
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn signing_input(header: &serde_json::Value, claims: &serde_json::Value) -> String {
    format!("{}.{}", b64(header.to_string().as_bytes()), b64(claims.to_string().as_bytes()))
}

/// An ES256 key (an Apple `.p8` file, PKCS#8 PEM).
pub struct Es256Key(EcdsaKeyPair);

impl Es256Key {
    pub fn from_pem(pem: &str) -> Result<Self> {
        let der = pem_to_der(pem)?;
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &der, &SystemRandom::new())
            .map_err(|e| anyhow::anyhow!("not a valid P-256 PKCS#8 private key: {e}"))?;
        Ok(Self(key))
    }

    /// `header.claims.signature` with `alg` ES256 and the given `kid`.
    pub fn sign(&self, kid: &str, claims: &serde_json::Value) -> Result<String> {
        let input = signing_input(&serde_json::json!({ "alg": "ES256", "kid": kid }), claims);
        let sig = self.0.sign(&SystemRandom::new(), input.as_bytes()).map_err(|_| anyhow::anyhow!("signing failed"))?;
        Ok(format!("{input}.{}", b64(sig.as_ref())))
    }
}

/// An RS256 key (a Google service account's `private_key`, PKCS#8 PEM).
pub struct Rs256Key(RsaKeyPair);

impl Rs256Key {
    pub fn from_pem(pem: &str) -> Result<Self> {
        let der = pem_to_der(pem)?;
        let key = RsaKeyPair::from_pkcs8(&der).map_err(|e| anyhow::anyhow!("not a valid RSA PKCS#8 private key: {e}"))?;
        Ok(Self(key))
    }

    pub fn sign(&self, claims: &serde_json::Value) -> Result<String> {
        let input = signing_input(&serde_json::json!({ "alg": "RS256", "typ": "JWT" }), claims);
        let mut sig = vec![0u8; self.0.public().modulus_len()];
        self.0
            .sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), input.as_bytes(), &mut sig)
            .map_err(|_| anyhow::anyhow!("signing failed"))?;
        Ok(format!("{input}.{}", b64(&sig)))
    }
}

/// Splits a JWT into its decoded header, claims and raw signature (tests).
#[cfg(test)]
pub fn decode(jwt: &str) -> (serde_json::Value, serde_json::Value, Vec<u8>, String) {
    let parts: Vec<&str> = jwt.split('.').collect();
    assert_eq!(parts.len(), 3, "a JWT has three parts");
    let json = |s: &str| serde_json::from_slice(&URL_SAFE_NO_PAD.decode(s).unwrap()).unwrap();
    (json(parts[0]), json(parts[1]), URL_SAFE_NO_PAD.decode(parts[2]).unwrap(), format!("{}.{}", parts[0], parts[1]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::testkeys::{EC_P8, RSA_PEM};
    use ring::signature::{ECDSA_P256_SHA256_FIXED, KeyPair, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};

    #[test]
    fn es256_token_has_apns_shape_and_verifies() {
        let key = Es256Key::from_pem(EC_P8).unwrap();
        let jwt = key.sign("KEY1234567", &serde_json::json!({ "iss": "TEAM123456", "iat": 1700000000 })).unwrap();
        let (header, claims, sig, input) = decode(&jwt);
        assert_eq!(header, serde_json::json!({ "alg": "ES256", "kid": "KEY1234567" }));
        assert_eq!(claims, serde_json::json!({ "iss": "TEAM123456", "iat": 1700000000 }));
        assert_eq!(sig.len(), 64, "raw r||s, not DER");
        // Verify with the public key derived from the same private key.
        let der = pem_to_der(EC_P8).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &der, &SystemRandom::new()).unwrap();
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, pair.public_key().as_ref()).verify(input.as_bytes(), &sig).unwrap();
    }

    #[test]
    fn rs256_token_has_oauth_shape_and_verifies() {
        let key = Rs256Key::from_pem(RSA_PEM).unwrap();
        let jwt = key.sign(&serde_json::json!({ "iss": "sa@p.iam.gserviceaccount.com", "scope": "s" })).unwrap();
        let (header, claims, sig, input) = decode(&jwt);
        assert_eq!(header, serde_json::json!({ "alg": "RS256", "typ": "JWT" }));
        assert_eq!(claims["iss"], "sa@p.iam.gserviceaccount.com");
        assert_eq!(sig.len(), 256);
        let der = pem_to_der(RSA_PEM).unwrap();
        let pair = RsaKeyPair::from_pkcs8(&der).unwrap();
        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, pair.public().as_ref()).verify(input.as_bytes(), &sig).unwrap();
    }

    #[test]
    fn rejects_garbage_keys() {
        assert!(Es256Key::from_pem("").is_err());
        assert!(Es256Key::from_pem("-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----").is_err());
        assert!(Rs256Key::from_pem(EC_P8).is_err());
    }
}
