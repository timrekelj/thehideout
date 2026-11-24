// The web voice engine: a thin layer over the `livekit-client` SDK that speaks the same language
// as the native engine (`thehideout-voice`): commands in, `VoiceEvent`s out. Rust owns the app's
// state; this file owns the browser's audio: the room, the microphone, the speakers, the level
// meter and the audio cues. Events go to Rust as JSON strings (see `voice_web.rs`).
//
// The SDK is not in the wasm bundle: it is loaded on first use from the file `build.sh` puts at
// the bundle's root (pinned version, checked checksum), so a page that never joins voice never
// fetches it. Everything is same-origin: nothing here needs more than the CSP's `script-src 'self'`
// and `connect-src` to the LiveKit server.

let sdk;
async function loadSdk() {
    // snippets/<crate>-<hash>/js/voice.js -> the bundle root.
    sdk ??= await import(new URL("../../../livekit-client.esm.mjs", import.meta.url).href);
    return sdk;
}

const LEVELS_MS = 66; // about 15 Hz, as the native engine
const LEVEL_FLOOR_DB = -60;
const VAD_HANGOVER_MS = 400;

const toLevel = (amplitude) => {
    if (!(amplitude > 0)) return 0;
    return Math.min(1, Math.max(0, (20 * Math.log10(amplitude) - LEVEL_FLOOR_DB) / -LEVEL_FLOOR_DB));
};

let audioContext;
let cueSink = ""; // the output device the cues play on ("" = the default)
/** The page's one AudioContext (cues and meters). Resumed from user gestures. */
function context() {
    if (!audioContext) {
        audioContext = new (window.AudioContext || window.webkitAudioContext)();
        routeCues();
    }
    return audioContext;
}

/** Plays the cues on the chosen output where the browser can (AudioContext.setSinkId); else the default. */
function routeCues() {
    if (typeof audioContext?.setSinkId !== "function") return;
    audioContext.setSinkId(cueSink).catch(() => { /* a vanished device: keep the current one */ });
}

/** Call from a user gesture (the join click): lets the page play audio from then on. */
export function unlockAudio() {
    try {
        const ctx = context();
        if (ctx.state === "suspended") void ctx.resume();
    } catch { /* no WebAudio */ }
}

/** Plays mono float samples once (the join, leave and mute cues). */
export function playPcm(samples, rate) {
    console.debug("voice-cmd", "cue", samples.length);
    try {
        const ctx = context();
        if (ctx.state === "suspended") void ctx.resume();
        const buffer = ctx.createBuffer(1, samples.length, rate);
        buffer.copyToChannel(samples, 0);
        const source = ctx.createBufferSource();
        source.buffer = buffer;
        source.connect(ctx.destination);
        source.start();
    } catch { /* a missing cue is not worth an error */ }
}

/** RMS of an analyser's current frame, mapped to the meter scale. */
function levelOf(analyser, buf) {
    analyser.getFloatTimeDomainData(buf);
    let sum = 0;
    for (const s of buf) sum += s * s;
    return toLevel(Math.sqrt(sum / buf.length));
}

/** A level meter on a MediaStreamTrack (a clone, so muting the original does not blind it). */
class Meter {
    constructor(track) {
        this.track = track.clone();
        this.track.enabled = true; // a clone starts as the original is: the gate may have closed it
        const ctx = context();
        this.source = ctx.createMediaStreamSource(new MediaStream([this.track]));
        this.analyser = ctx.createAnalyser();
        this.analyser.fftSize = 1024;
        this.source.connect(this.analyser);
        this.buf = new Float32Array(this.analyser.fftSize);
    }
    level() {
        return levelOf(this.analyser, this.buf);
    }
    close() {
        this.source.disconnect();
        this.track.stop();
    }
}

export class VoiceClient {
    /** `emit(json)` receives every event as a JSON string. */
    constructor(emit) {
        this.lastError = { message: "", at: 0 };
        this.emit = (event) => {
            if (event.t === "error") {
                // The SDK and our own call often report the same failure: say it once.
                const now = performance.now();
                if (event.message === this.lastError.message && now - this.lastError.at < 2000) return;
                this.lastError = { message: event.message, at: now };
            }
            const json = JSON.stringify(event);
            // Levels arrive 15 times a second; everything else is worth a (debug-level) line.
            if (event.t !== "levels") console.debug("voice", json);
            else if (event.levels.some(([, l]) => l > 0)) console.debug("voice-levels-nonzero");
            emit(json);
        };
        this.room = undefined;
        this.muted = false;
        this.deaf = false;
        this.volumes = new Map(); // identity -> 0..1 ("mute for me" is 0)
        this.input = undefined; // device id, or undefined for the default
        this.output = undefined;
        this.ns = true;
        this.ec = true;
        this.ptt = false;
        this.pttMode = false;
        this.threshold = 0.35;
        this.meter = undefined; // our microphone, in a room
        this.testMeter = undefined; // the settings page's mic test, no room
        this.testStream = undefined;
        this.micStale = false;
        this.vadOpenUntil = 0;
        this.lastLevelsLoud = false;
        this.timer = undefined;
        this.banner = undefined;
        window.addEventListener("blur", () => this.releasePtt());
        document.addEventListener("visibilitychange", () => {
            if (document.visibilityState === "hidden") this.releasePtt();
        });
        if (navigator.mediaDevices?.addEventListener) {
            navigator.mediaDevices.addEventListener("devicechange", () => {
                if (this.wantDevices) void this.listDevices();
            });
        }
    }

    // ---- the room ----------------------------------------------------------------------------

    /**
     * A connect is a session: `{ room, reported }`. The Rust sink counts exactly one `disconnected`
     * per teardown it asks for (a `disconnect`, or a `connect` while one is active), whatever state
     * the session was in (loading the SDK, connecting, joined). So a session's end is reported once:
     * by `end` (asked for), by livekit's `Disconnected` (dropped), or by a failed connect, and
     * `reported` makes sure the later ones stay quiet.
     */
    async connect(url, token, startMuted, startDeaf) {
        const previous = this.session;
        const session = { room: undefined, reported: false };
        this.session = session;
        const current = () => this.session === session;
        try {
            if (previous) await this.end(previous);
            if (!current()) return; // left (or replaced) meanwhile: `end` has reported it
            const lk = await loadSdk();
            if (!current()) return; // the user left while the SDK loaded: never join
            lk.setLogLevel("warn");
            this.muted = startMuted || startDeaf;
            this.deaf = startDeaf;
            const room = new lk.Room({
                adaptiveStream: false,
                dynacast: false,
                audioCaptureDefaults: this.captureOptions(),
                // The mic is released while muted (the browser's recording indicator goes off).
                publishDefaults: { stopMicTrackOnMute: true },
                audioOutput: this.output ? { deviceId: this.output } : undefined,
                stopLocalTrackOnUnpublish: true,
            });
            session.room = room;
            this.room = room;
            this.wire(room, lk, session);
            this.emit({ t: "connecting" });
            await room.connect(url, token, { autoSubscribe: true });
            if (!current()) {
                await room.disconnect(true).catch(() => {});
                return;
            }
            this.emit({ t: "connected" });
            for (const p of room.remoteParticipants.values()) {
                this.watch(p, lk);
                this.applyVolume(p);
                this.emit({ t: "participantJoined", identity: p.identity });
            }
            this.watch(room.localParticipant, lk);
            this.startLevels();
            // The user's click that started the join lets the page play from here on.
            try { await room.startAudio(); } catch { /* the banner offers it */ }
            if (!current()) return;
            await this.applyMute(true);
        } catch (error) {
            // A failure before or during the join (SDK load, CSP, a refused token). When livekit
            // already reported the end (it does for a failed connect), or the user left, say nothing.
            if (!current() || session.reported) return;
            this.finish(session, describe(error));
            await session.room?.disconnect(true).catch(() => {});
        }
    }

    /** Reports a session's end once and drops what it owned. Returns whether it was the one to say so. */
    finish(session, reason) {
        const first = !session.reported;
        session.reported = true;
        if (this.session === session) this.session = undefined;
        if (this.room === session.room) {
            this.room = undefined;
            this.stopRoomAudio();
        }
        this.hideBanner();
        if (first) this.emit({ t: "disconnected", reason });
        return first;
    }

    /** Ends a session on request: reports it, then leaves its room (if it got that far). */
    async end(session) {
        const room = session.room;
        this.finish(session, "left");
        if (room) await room.disconnect(true).catch(() => {});
    }

    wire(room, lk, session) {
        const { RoomEvent } = lk;
        room
            .on(RoomEvent.ParticipantConnected, (p) => {
                this.watch(p, lk);
                this.applyVolume(p);
                this.emit({ t: "participantJoined", identity: p.identity });
            })
            .on(RoomEvent.ParticipantDisconnected, (p) => {
                this.emit({ t: "participantLeft", identity: p.identity });
            })
            .on(RoomEvent.TrackSubscribed, (track, _pub, p) => {
                if (track.kind === lk.Track.Kind.Audio) {
                    this.applyVolume(p);
                    track.attach(); // an <audio> element playing it, held by the track
                }
            })
            .on(RoomEvent.TrackMuted, (pub, p) => this.trackMuted(pub, p, true, lk))
            .on(RoomEvent.TrackUnmuted, (pub, p) => this.trackMuted(pub, p, false, lk))
            .on(RoomEvent.ConnectionQualityChanged, (quality, p) => {
                const q = { excellent: "excellent", good: "good", poor: "poor", lost: "lost" }[quality];
                if (q) this.emit({ t: "quality", identity: p.identity, quality: q });
            })
            .on(RoomEvent.Reconnecting, () => this.emit({ t: "reconnecting" }))
            .on(RoomEvent.Reconnected, () => this.emit({ t: "connected" }))
            .on(RoomEvent.AudioPlaybackStatusChanged, () => this.audioBanner(room));
        room.on(RoomEvent.Disconnected, (reason) => this.finish(session, reasonText(lk, reason)));
        // A device unplugged or a permission revoked while in the room.
        room.on(RoomEvent.MediaDevicesError, (error) => this.emit({ t: "error", message: micMessage(error) }));
    }

    watch(p, lk) {
        if (p._thWatched) return;
        p._thWatched = true;
        p.on(lk.ParticipantEvent.IsSpeakingChanged, (speaking) => this.emit({ t: "speaking", identity: p.identity, speaking }));
        // Changes only: someone already talking when we arrive would never be reported otherwise.
        if (p.isSpeaking) this.emit({ t: "speaking", identity: p.identity, speaking: true });
    }

    trackMuted(pub, p, muted, lk) {
        if (pub.kind === lk.Track.Kind.Audio) this.emit({ t: "trackMuted", identity: p.identity, muted });
    }

    disconnect() {
        const session = this.session;
        if (session) void this.end(session);
    }

    stopRoomAudio() {
        this.meter?.close();
        this.meter = undefined;
        if (!this.testMeter) this.stopLevels();
    }

    // ---- audio playback (autoplay) ---------------------------------------------------------

    audioBanner(room) {
        if (room.canPlaybackAudio) return this.hideBanner();
        if (this.banner) return;
        const button = document.createElement("button");
        button.textContent = "Click to enable audio";
        button.setAttribute("style", "position:fixed;left:50%;bottom:72px;transform:translateX(-50%);z-index:10;padding:10px 18px;border:0;border-radius:10px;font:600 14px system-ui;background:#0f766e;color:#fff;cursor:pointer;box-shadow:0 2px 10px rgba(0,0,0,.3)");
        button.addEventListener("click", () => {
            unlockAudio();
            room.startAudio().then(() => this.hideBanner(), () => {});
        });
        document.body.appendChild(button);
        this.banner = button;
    }

    hideBanner() {
        this.banner?.remove();
        this.banner = undefined;
    }

    // ---- microphone --------------------------------------------------------------------------

    captureOptions() {
        return {
            echoCancellation: this.ec,
            noiseSuppression: this.ns,
            autoGainControl: true,
            deviceId: this.input,
        };
    }

    micTrack() {
        const lk = sdk;
        const pub = this.room?.localParticipant.getTrackPublication(lk.Track.Source.Microphone);
        return pub?.audioTrack;
    }

    /** Whether the microphone should be sending right now (mute, push to talk and voice activity). */
    gateOpen() {
        if (this.muted || this.deaf) return false;
        if (this.pttMode) return this.ptt;
        return performance.now() < this.vadOpenUntil || this.threshold <= 0;
    }

    /** Publishes or mutes the microphone; `first` is the connect-time call. */
    async applyMute(first = false) {
        const lp = this.room?.localParticipant;
        if (!lp) return;
        try {
            if (this.muted || this.deaf) {
                // Muted: nothing listens, so the meter's clone goes too (it would keep the mic,
                // and the browser's recording indicator, on).
                this.meter?.close();
                this.meter = undefined;
                if (lp.isMicrophoneEnabled) await lp.setMicrophoneEnabled(false);
            } else if (!lp.isMicrophoneEnabled) {
                await lp.setMicrophoneEnabled(true, this.captureOptions());
                const track = this.micTrack()?.mediaStreamTrack;
                if (this.micStale) {
                    // Settings changed while muted (the mic was released): reopen with them.
                    this.micStale = false;
                    await this.restartMic();
                } else if (track) {
                    this.meter?.close();
                    this.meter = new Meter(track);
                }
            }
            this.applyGate();
        } catch (error) {
            this.emit({ t: "error", message: micMessage(error) });
            if (first) await lp.setMicrophoneEnabled(false).catch(() => {});
        }
    }

    applyGate() {
        const track = this.micTrack()?.mediaStreamTrack;
        if (track) track.enabled = this.gateOpen();
    }

    async restartMic() {
        const track = this.micTrack();
        if (!track) return;
        if (this.muted || this.deaf) {
            this.micStale = true; // the mic is off: do not open it just to apply a setting
            return;
        }
        try {
            await track.restartTrack(this.captureOptions());
            this.meter?.close();
            this.meter = new Meter(track.mediaStreamTrack);
            this.applyGate();
        } catch (error) {
            this.emit({ t: "error", message: micMessage(error) });
        }
    }

    setMute(muted) {
        this.muted = muted;
        void this.applyMute();
    }

    setDeaf(deaf) {
        this.deaf = deaf;
        this.room?.remoteParticipants.forEach((p) => this.applyVolume(p));
        void this.applyMute();
    }

    setUserVolume(identity, volume) {
        console.debug("voice-cmd", "volume", identity, volume);
        this.volumes.set(identity, volume);
        const p = this.room?.remoteParticipants.get(identity);
        if (p) this.applyVolume(p);
    }

    applyVolume(p) {
        const v = this.deaf ? 0 : (this.volumes.get(p.identity) ?? 1);
        try { p.setVolume(v); } catch { /* not subscribed yet: TrackSubscribed applies it */ }
    }

    setMode(vad, threshold) {
        this.pttMode = !vad;
        this.threshold = threshold;
        this.applyGate();
    }

    setPtt(held) {
        console.debug("voice-cmd", "ptt", held);
        this.ptt = held;
        this.applyGate();
    }

    /** A key-up lost to a focus change never arrives: close the gate (as the desktop app does). */
    releasePtt() {
        if (!this.ptt) return;
        console.debug("voice-cmd", "ptt-release");
        this.setPtt(false);
    }

    setNoiseSuppression(on) {
        this.ns = on;
        void this.restartMic();
    }

    setEchoCancellation(on) {
        this.ec = on;
        void this.restartMic();
    }

    async setInputDevice(id) {
        console.debug("voice-cmd", "input", id ?? "default");
        this.input = id ?? undefined;
        if (this.room && this.micTrack() && (this.muted || this.deaf)) {
            this.micStale = true;
        } else if (this.room && this.micTrack()) {
            try {
                const device = await this.deviceFor("audioinput", id);
                if (device) await this.room.switchActiveDevice("audioinput", device);
                const track = this.micTrack();
                if (track) {
                    this.meter?.close();
                    this.meter = new Meter(track.mediaStreamTrack);
                }
            } catch (error) {
                this.emit({ t: "error", message: micMessage(error) });
            }
        }
        if (this.testMeter) await this.startMicTest();
    }

    /**
     * The device id to switch to: the chosen one, or for "Default" the browser's "default" alias
     * (Chromium) or else the first device (other browsers have no alias); undefined to leave it be.
     */
    async deviceFor(kind, id) {
        if (id) return id;
        const list = await sdk.Room.getLocalDevices(kind, false);
        return list.find((d) => d.deviceId === "default")?.deviceId ?? list[0]?.deviceId;
    }

    async setOutputDevice(id) {
        console.debug("voice-cmd", "output", id ?? "default");
        this.output = id ?? undefined;
        cueSink = id ?? "";
        routeCues();
        if (this.room) {
            try {
                const device = await this.deviceFor("audiooutput", id);
                if (device) await this.room.switchActiveDevice("audiooutput", device);
            } catch (error) { this.emit({ t: "error", message: `Couldn't switch the speakers: ${describe(error)}` }); }
        }
    }

    // ---- devices and the mic test ---------------------------------------------------------

    async listDevices() {
        this.wantDevices = true;
        const lk = await loadSdk();
        try {
            // Labels need a permission; ask once, here, so the lists are named.
            const inputs = await lk.Room.getLocalDevices("audioinput", true);
            const outputs = await lk.Room.getLocalDevices("audiooutput", false);
            const map = (d) => ({ id: d.deviceId, name: d.label || d.deviceId });
            const real = (d) => d.deviceId !== "default" && d.deviceId !== "communications";
            // The browser lists a "default" alias; the device it points at (same group) is the default.
            const defaultOf = (list) => {
                const alias = list.find((d) => d.deviceId === "default");
                return list.filter(real).find((d) => alias && d.groupId === alias.groupId)?.deviceId;
            };
            this.emit({
                t: "devices",
                inputs: inputs.filter(real).map(map),
                outputs: outputs.filter(real).map(map),
                defaultInput: defaultOf(inputs),
                defaultOutput: defaultOf(outputs),
            });
        } catch (error) {
            this.emit({ t: "error", message: micMessage(error) });
        }
    }

    async startMicTest() {
        this.stopMicTestStream();
        try {
            this.testStream = await navigator.mediaDevices.getUserMedia({ audio: this.captureOptions() });
            this.testMeter = new Meter(this.testStream.getAudioTracks()[0]);
            this.startLevels();
        } catch (error) {
            this.emit({ t: "error", message: micMessage(error) });
        }
    }

    stopMicTestStream() {
        this.testMeter?.close();
        this.testMeter = undefined;
        this.testStream?.getTracks().forEach((t) => t.stop());
        this.testStream = undefined;
    }

    stopMicTest() {
        this.stopMicTestStream();
        if (!this.room) this.stopLevels();
    }

    // ---- levels (the meters, and voice activity) -----------------------------------------

    startLevels() {
        if (this.timer) return;
        this.timer = setInterval(() => this.tick(), LEVELS_MS);
    }

    stopLevels() {
        clearInterval(this.timer);
        this.timer = undefined;
        if (this.lastLevelsLoud) this.emit({ t: "levels", levels: [] });
        this.lastLevelsLoud = false;
    }

    tick() {
        const levels = [];
        const mine = this.meter ?? this.testMeter;
        let local = 0;
        if (mine) {
            local = mine.level();
            levels.push([this.room?.localParticipant.identity ?? "", local]);
        }
        // Voice activity: open while the mic is above the threshold, and a moment after.
        if (this.meter && !this.pttMode && local >= this.threshold) this.vadOpenUntil = performance.now() + VAD_HANGOVER_MS;
        if (this.meter) this.applyGate();
        for (const p of this.room?.remoteParticipants.values() ?? []) levels.push([p.identity, toLevel(p.audioLevel)]);
        const loud = levels.some(([, l]) => l > 0);
        if (loud || this.lastLevelsLoud) this.emit({ t: "levels", levels: loud ? levels : levels.map(([id]) => [id, 0]) });
        this.lastLevelsLoud = loud;
    }
}

function describe(error) {
    return error?.message ? String(error.message) : String(error);
}

function micMessage(error) {
    const name = error?.name ?? "";
    if (name === "NotAllowedError" || name === "SecurityError" || /permission|denied|not allowed/i.test(describe(error))) {
        return "Microphone access was denied. Allow it for this site in your browser's address bar, then join again.";
    }
    if (name === "NotFoundError" || name === "OverconstrainedError") return "No microphone was found.";
    if (name === "NotReadableError") return "The microphone is in use by another app.";
    return `Microphone problem: ${describe(error)}`;
}

function reasonText(lk, reason) {
    const name = lk.DisconnectReason ? Object.entries(lk.DisconnectReason).find(([, v]) => v === reason)?.[0] : undefined;
    return name ? name.replace(/_/g, " ").toLowerCase() : "disconnected";
}
