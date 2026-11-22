//! The client address behind a reverse proxy.
//!
//! Rate limits and the access log key on the client IP. Directly connected,
//! that is the socket's peer address. Behind a proxy it is the proxy's, so
//! `THEHIDEOUT_TRUSTED_PROXIES` lists the proxies (IPs / CIDRs) whose
//! `X-Forwarded-For` / `X-Real-IP` headers are believed. Headers of any other
//! peer are ignored, so a client cannot choose its own rate-limit key.

use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{Extensions, HeaderMap};
use axum::middleware::Next;
use axum::response::Response;

/// The resolved client address, stored in request extensions by [`middleware`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

/// An IP network: `10.0.0.0/8`, `::1/128`, or a single address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

/// IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`) compare as IPv4.
fn canonical(ip: IpAddr) -> IpAddr {
    ip.to_canonical()
}

impl FromStr for Cidr {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim();
        let (addr, prefix) = match s.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (s, None),
        };
        let addr: IpAddr = addr.trim().parse().with_context(|| format!("invalid IP address in {s:?}"))?;
        let addr = canonical(addr);
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(p) => p.trim().parse::<u8>().with_context(|| format!("invalid prefix length in {s:?}"))?,
            None => max,
        };
        if prefix > max {
            bail!("prefix length in {s:?} is over {max}");
        }
        Ok(Self { addr, prefix })
    }
}

impl Cidr {
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, canonical(ip)) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX.checked_shl(32 - u32::from(self.prefix)).unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX.checked_shl(128 - u32::from(self.prefix)).unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

/// The proxies whose forwarding headers are trusted (`THEHIDEOUT_TRUSTED_PROXIES`).
/// Empty (the default) trusts none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<Cidr>);

impl TrustedProxies {
    /// A comma-separated list of IPs and CIDRs; blank = none.
    pub fn parse(list: &str) -> Result<Self> {
        let mut nets = Vec::new();
        for item in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            nets.push(item.parse().context("invalid THEHIDEOUT_TRUSTED_PROXIES")?);
        }
        Ok(Self(nets))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        self.0.iter().any(|n| n.contains(ip))
    }
}

/// One `X-Forwarded-For` / `X-Real-IP` entry: `ip`, `ip:port`, `[v6]`, `[v6]:port`.
fn parse_entry(s: &str) -> Option<IpAddr> {
    let s = s.trim();
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Some(ip);
    }
    if let Ok(sock) = s.parse::<SocketAddr>() {
        return Some(sock.ip());
    }
    s.strip_prefix('[')?.strip_suffix(']')?.parse().ok()
}

/// The client address of a request received from `peer`.
///
/// An untrusted peer (or no trusted proxies at all) is the client. From a
/// trusted peer the client is the right-most `X-Forwarded-For` address that is
/// not itself a trusted proxy (what the nearest proxy saw), else `X-Real-IP`,
/// else the peer.
pub fn client_ip(trusted: &TrustedProxies, peer: IpAddr, headers: &HeaderMap) -> IpAddr {
    let peer = canonical(peer);
    if !trusted.contains(peer) {
        return peer;
    }
    let forwarded: Vec<&str> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .collect();
    for entry in forwarded.into_iter().rev() {
        match parse_entry(entry) {
            Some(ip) if trusted.contains(ip) => continue,
            Some(ip) => return canonical(ip),
            // Garbage in the chain: nothing to its left can be believed.
            None => break,
        }
    }
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_entry)
        .filter(|ip| !trusted.contains(*ip))
        .map_or(peer, canonical)
}

/// The client address stored for a request, falling back to the socket peer
/// (requests that never went through [`middleware`]).
pub fn from_extensions(ext: &Extensions) -> Option<IpAddr> {
    ext.get::<ClientIp>()
        .map(|c| c.0)
        .or_else(|| ext.get::<ConnectInfo<SocketAddr>>().map(|c| canonical(c.ip())))
}

/// Resolves the client address and stores it as [`ClientIp`].
pub async fn middleware(State(trusted): State<Arc<TrustedProxies>>, mut request: Request, next: Next) -> Response {
    let peer = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.ip());
    if let Some(peer) = peer {
        let ip = client_ip(&trusted, peer, request.headers());
        request.extensions_mut().insert(ClientIp(ip));
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    fn trusted(s: &str) -> TrustedProxies {
        TrustedProxies::parse(s).unwrap()
    }

    #[test]
    fn parses_lists_of_ips_and_cidrs() {
        assert!(trusted("").is_empty());
        assert!(trusted(" , ").is_empty());
        let t = trusted("127.0.0.1/32, 172.16.0.0/12,::1,fd00::/8");
        assert!(t.contains(ip("127.0.0.1")));
        assert!(!t.contains(ip("127.0.0.2")));
        assert!(t.contains(ip("172.31.255.255")));
        assert!(!t.contains(ip("172.32.0.1")));
        assert!(t.contains(ip("::1")));
        assert!(t.contains(ip("fd12::5")));
        assert!(!t.contains(ip("fe80::1")));
        // A v4-mapped v6 peer matches a v4 network.
        assert!(t.contains(ip("::ffff:172.17.0.2")));
        // /0 matches everything of its family.
        assert!(trusted("0.0.0.0/0").contains(ip("8.8.8.8")));
        assert!(!trusted("0.0.0.0/0").contains(ip("2001:db8::1")));
    }

    #[test]
    fn rejects_bad_entries() {
        for bad in ["nonsense", "10.0.0.0/33", "::/129", "10.0.0.0/x", "10.0.0/8", "10.0.0.1/-1"] {
            assert!(TrustedProxies::parse(bad).is_err(), "{bad}");
        }
        assert!(TrustedProxies::parse("10.0.0.0/8,oops").is_err());
    }

    #[test]
    fn no_trusted_proxies_uses_the_socket_ip() {
        let h = headers(&[("x-forwarded-for", "1.2.3.4"), ("x-real-ip", "5.6.7.8")]);
        assert_eq!(client_ip(&TrustedProxies::default(), ip("203.0.113.9"), &h), ip("203.0.113.9"));
    }

    #[test]
    fn spoofed_headers_from_an_untrusted_peer_are_ignored() {
        let t = trusted("10.0.0.0/8");
        let h = headers(&[("x-forwarded-for", "1.2.3.4"), ("x-real-ip", "5.6.7.8")]);
        assert_eq!(client_ip(&t, ip("203.0.113.9"), &h), ip("203.0.113.9"));
    }

    #[test]
    fn a_trusted_peer_gives_the_forwarded_client() {
        let t = trusted("10.0.0.0/8");
        let h = headers(&[("x-forwarded-for", "198.51.100.7")]);
        assert_eq!(client_ip(&t, ip("10.0.0.2"), &h), ip("198.51.100.7"));
    }

    #[test]
    fn the_rightmost_untrusted_hop_wins_so_a_client_cannot_prepend_one() {
        let t = trusted("10.0.0.0/8");
        // The client sent "6.6.6.6"; the proxy appended the address it saw, then
        // an inner proxy appended its own hop.
        let h = headers(&[("x-forwarded-for", "6.6.6.6, 198.51.100.7, 10.0.0.3")]);
        assert_eq!(client_ip(&t, ip("10.0.0.2"), &h), ip("198.51.100.7"));
        // Several header lines count as one list.
        let h = headers(&[("x-forwarded-for", "6.6.6.6"), ("x-forwarded-for", "198.51.100.7, 10.0.0.3")]);
        assert_eq!(client_ip(&t, ip("10.0.0.2"), &h), ip("198.51.100.7"));
    }

    #[test]
    fn falls_back_to_real_ip_then_the_peer() {
        let t = trusted("10.0.0.0/8");
        let h = headers(&[("x-real-ip", "198.51.100.8")]);
        assert_eq!(client_ip(&t, ip("10.0.0.2"), &h), ip("198.51.100.8"));
        // Only trusted hops in the chain: X-Real-IP decides.
        let h = headers(&[("x-forwarded-for", "10.0.0.9, 10.0.0.3"), ("x-real-ip", "198.51.100.8")]);
        assert_eq!(client_ip(&t, ip("10.0.0.2"), &h), ip("198.51.100.8"));
        assert_eq!(client_ip(&t, ip("10.0.0.2"), &HeaderMap::new()), ip("10.0.0.2"));
        // Garbage is not believed.
        let h = headers(&[("x-forwarded-for", "not-an-ip"), ("x-real-ip", "also-bad")]);
        assert_eq!(client_ip(&t, ip("10.0.0.2"), &h), ip("10.0.0.2"));
        // Garbage in the chain hides what is left of it.
        let h = headers(&[("x-forwarded-for", "6.6.6.6, junk, 198.51.100.7")]);
        assert_eq!(client_ip(&t, ip("10.0.0.2"), &h), ip("198.51.100.7"));
        let h = headers(&[("x-forwarded-for", "6.6.6.6, junk")]);
        assert_eq!(client_ip(&t, ip("10.0.0.2"), &h), ip("10.0.0.2"));
    }

    #[test]
    fn ipv6_clients_proxies_ports_and_mapped_addresses() {
        let t = trusted("::1, 172.16.0.0/12");
        let h = headers(&[("x-forwarded-for", "2001:db8::42, 172.18.0.5")]);
        assert_eq!(client_ip(&t, ip("::1"), &h), ip("2001:db8::42"));
        let h = headers(&[("x-forwarded-for", "[2001:db8::43]:5555")]);
        assert_eq!(client_ip(&t, ip("172.18.0.2"), &h), ip("2001:db8::43"));
        let h = headers(&[("x-forwarded-for", "203.0.113.5:4711")]);
        assert_eq!(client_ip(&t, ip("172.18.0.2"), &h), ip("203.0.113.5"));
        // A v4 peer arriving as ::ffff:a.b.c.d on a dual-stack socket.
        let h = headers(&[("x-forwarded-for", "203.0.113.6")]);
        assert_eq!(client_ip(&t, ip("::ffff:172.18.0.2"), &h), ip("203.0.113.6"));
        assert_eq!(client_ip(&TrustedProxies::default(), ip("::ffff:192.0.2.1"), &h), ip("192.0.2.1"));
    }
}
