// Browser glue for the thehideout web client: the few DOM and WebAuthn calls Rust can't make
// directly (or that are clearer here). Served as a module snippet next to app.js; no eval, so
// it works under a strict CSP.

const b64 = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const unb64 = (s) => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));

export function origin() {
    return location.origin;
}

export function pathname() {
    return location.pathname;
}

export function search() {
    return location.search;
}

/** Replaces the address-bar path without a reload or a history entry. */
export function replacePath(path) {
    try { history.replaceState(null, "", path); } catch { /* the address bar just keeps the old path */ }
}

export function storageGet(key) {
    try { return localStorage.getItem(key) ?? undefined; } catch { return undefined; }
}

export function storageSet(key, value) {
    try { localStorage.setItem(key, value); } catch { /* private mode or full: the value only lasts this visit */ }
}

export function storageRemove(key) {
    try { localStorage.removeItem(key); } catch { /* as above */ }
}

export function userAgent() {
    return navigator.userAgent;
}

/** Opens the file chooser. Call it from an input handler so the browser sees the user gesture. */
export function pickFiles(accept, multiple) {
    return new Promise((resolve) => {
        const input = document.createElement("input");
        input.type = "file";
        input.accept = accept;
        input.multiple = multiple;
        input.style.display = "none";
        input.addEventListener("change", () => { resolve(Array.from(input.files ?? [])); input.remove(); });
        input.addEventListener("cancel", () => { resolve([]); input.remove(); });
        document.body.appendChild(input);
        input.click();
    });
}

export async function readBlob(blob) {
    return new Uint8Array(await blob.arrayBuffer());
}

function save(href, name, revoke) {
    const a = document.createElement("a");
    a.href = href;
    a.download = name;
    a.style.display = "none";
    document.body.appendChild(a);
    a.click();
    a.remove();
    if (revoke) setTimeout(() => URL.revokeObjectURL(href), 10000);
}

export function downloadText(name, text) {
    save(URL.createObjectURL(new Blob([text], { type: "text/plain" })), name, true);
}

export function downloadUrl(url, name) {
    save(url, name, false);
}

/** `navigator.credentials.create` for the server's `options_json`; resolves to the credential as JSON. */
export async function passkeyCreate(optionsJson) {
    const pk = JSON.parse(optionsJson).publicKey;
    pk.challenge = unb64(pk.challenge);
    pk.user.id = unb64(pk.user.id);
    (pk.excludeCredentials || []).forEach((c) => (c.id = unb64(c.id)));
    const cred = await navigator.credentials.create({ publicKey: pk });
    return JSON.stringify({
        id: cred.id, rawId: b64(cred.rawId), type: cred.type, extensions: cred.getClientExtensionResults(),
        response: {
            attestationObject: b64(cred.response.attestationObject), clientDataJSON: b64(cred.response.clientDataJSON),
            transports: cred.response.getTransports ? cred.response.getTransports() : [],
        },
    });
}

/** `navigator.credentials.get` for the server's `options_json`; resolves to the credential as JSON. */
export async function passkeyGet(optionsJson) {
    const pk = JSON.parse(optionsJson).publicKey;
    pk.challenge = unb64(pk.challenge);
    (pk.allowCredentials || []).forEach((c) => (c.id = unb64(c.id)));
    const cred = await navigator.credentials.get({ publicKey: pk });
    return JSON.stringify({
        id: cred.id, rawId: b64(cred.rawId), type: cred.type, extensions: cred.getClientExtensionResults(),
        response: {
            authenticatorData: b64(cred.response.authenticatorData), clientDataJSON: b64(cred.response.clientDataJSON),
            signature: b64(cred.response.signature), userHandle: cred.response.userHandle ? b64(cred.response.userHandle) : null,
        },
    });
}
