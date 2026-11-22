import Foundation

// The reducer: pure transitions of `AppState`. A port of desktop's `apply.rs` plus the state methods it
// calls (state.rs, moderation.rs, voice.rs, emoji.rs, structure.rs). Differences from desktop, all about
// navigation: a phone shows a channel only when its transcript is pushed, so `openChannel` is explicit
// (entering a server does not open its first channel) and a deleted open channel just closes.

extension AppState {
    // MARK: notices

    public mutating func notice(_ kind: Notice.Kind, _ text: String) {
        noticeCounter += 1
        notices.append(Notice(id: noticeCounter, kind: kind, text: text))
        if notices.count > Self.noticeQueue { notices.removeFirst(notices.count - Self.noticeQueue) }
    }

    public mutating func dismissNotice(_ id: Int) { notices.removeAll { $0.id == id } }

    // MARK: navigation

    public mutating func goHome() {
        place = .home
        closeChannel()
    }

    /// Enters a server: asks for its members, roles, emojis and (if missing) channels.
    public mutating func enterGuild(_ id: ID) {
        guard guild(id) != nil else { return }
        place = .guild(id)
        wantMembers(id)
        wantAccess(id)
        wantEmojis(id)
        if channels[id] == nil { outbox.append(.loadChannels(guild: id)) }
    }

    public mutating func wantMembers(_ guild: ID) {
        if members[guild] == nil, membersAsked.insert(guild).inserted { outbox.append(.loadMembers(guild: guild)) }
    }

    public mutating func wantAccess(_ guild: ID) {
        if roles[guild] == nil, accessAsked.insert(guild).inserted { outbox.append(.loadAccess(guild: guild)) }
    }

    public mutating func wantEmojis(_ guild: ID) {
        if emojis[guild] == nil, emojisAsked.insert(guild).inserted { outbox.append(.loadEmojis(guild: guild)) }
    }

    /// Asks for the members of the guild `m` was posted in when its author is a stranger.
    mutating func wantAuthor(_ m: Message) {
        if let g = m.guildID, users[m.authorID] == nil { wantMembers(g) }
    }

    /// Opens a text channel or DM. The reader just arrived: fix the "New" divider, clear the unread count and
    /// fetch the history if it hasn't been.
    public mutating func open(channel: ID) {
        openChannel = channel
        marker = .none
        let pending = unread.removeValue(forKey: channel).flatMap { $0.count > 0 ? $0 : nil }
        var log = logs[channel] ?? ChannelLog()
        if let u = pending {
            if log.loaded {
                let at = log.messages.count - (u.count + 1)
                marker = .after(channel: channel, id: at >= 0 ? log.messages[at].id : nil)
            } else {
                marker = .pending(channel: channel, unread: u.count)
            }
        }
        if !log.loaded, log.fetch == .idle {
            log.fetch = .initial
            outbox.append(.loadMessages(channel: channel, before: nil))
        } else if log.loaded, log.stale {
            // Keeps what is shown; the newest page merges in when it lands.
            log.stale = false
            outbox.append(.loadMessages(channel: channel, before: nil))
        }
        logs[channel] = log
    }

    public mutating func closeChannel() {
        openChannel = nil
        marker = .none
    }

    /// Tries the open channel's first page again after a failed load.
    public mutating func reloadOpen() {
        guard let channel = openChannel else { return }
        var log = logs[channel] ?? ChannelLog()
        if !log.loaded, log.fetch == .idle {
            log.fetch = .initial
            outbox.append(.loadMessages(channel: channel, before: nil))
        }
        logs[channel] = log
    }

    /// Asks for the page older than what's loaded, once.
    public mutating func loadOlder(_ channel: ID) {
        guard var log = logs[channel], log.loaded, log.hasMore, log.fetch == .idle else { return }
        log.fetch = .older
        outbox.append(.loadMessages(channel: channel, before: log.oldestID))
        logs[channel] = log
    }

    /// The gateway is back after a gap and doesn't replay: fetch the lists again, the open channel's newest page
    /// now and every other loaded channel's on its next visit. Nothing is cleared and the selection stays.
    public mutating func resync() {
        outbox.append(.refresh)
        for id in logs.keys { logs[id]!.stale = logs[id]!.loaded && id != openChannel }
        if let c = openChannel { outbox.append(.loadMessages(channel: c, before: nil)) }
    }

    public mutating func toggleSection(guild: ID, section: ID) {
        let k = SectionKey(guild: guild, section: section)
        if collapsed.contains(k) { collapsed.remove(k) } else { collapsed.insert(k) }
    }

    // MARK: sending

    /// Queues `text` as a pending message in `channel` (optimistic). False (and nothing queued) when the text is
    /// empty or over the limit.
    @discardableResult
    public mutating func send(_ text: String, to channel: ID, at now: Date, attachmentIDs: [ID] = []) -> Bool {
        let content = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard content.count <= Self.maxMessageChars, !(content.isEmpty && attachmentIDs.isEmpty) else { return false }
        nonceCounter += 1
        let nonce = "\(myID)-\(nonceCounter)"
        outgoing.append(Outgoing(nonce: nonce, channel: channel, content: content, at: now, attachmentIDs: attachmentIDs))
        outbox.append(.send(channel: channel, content: content, nonce: nonce, attachmentIDs: attachmentIDs))
        return true
    }

    public mutating func retry(nonce: String) {
        guard let i = outgoing.firstIndex(where: { $0.nonce == nonce && $0.state == .failed }) else { return }
        outgoing[i].state = .pending
        let o = outgoing[i]
        outbox.append(.send(channel: o.channel, content: o.content, nonce: o.nonce, attachmentIDs: o.attachmentIDs))
    }

    public mutating func discard(nonce: String) { outgoing.removeAll { $0.nonce == nonce } }

    /// Edits an own message in place (optimistic; a refusal rolls back, see `applyEditReply`). False (and
    /// nothing queued) when the message isn't loaded, or the new text is empty or over the limit.
    @discardableResult
    public mutating func editMessage(channel: ID, message: ID, content: String) -> Bool {
        let trimmed = content.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.count <= Self.maxMessageChars, !trimmed.isEmpty,
              let i = logs[channel]?.messages.firstIndex(where: { $0.id == message }) else { return false }
        let previous = logs[channel]!.messages[i].content
        guard previous != trimmed else { return false }
        logs[channel]!.messages[i].content = trimmed
        outbox.append(.editMessage(channel: channel, message: message, content: trimmed, previous: previous))
        return true
    }

    /// The reply to `editMessage`. Success: the server's version (attachments/embeds may differ once resolved).
    /// Failure: the edit rolls back to `previous` unless the message has moved on since (another edit, or it
    /// was deleted while editing).
    public mutating func applyEditReply(channel: ID, message: ID, previous: String, result: Result<Message, AppError>) {
        switch result {
        case .success(let m): messageUpdated(m)
        case .failure(let e):
            if let i = logs[channel]?.messages.firstIndex(where: { $0.id == message }) {
                logs[channel]!.messages[i].content = previous
            }
            notice(.danger, "Couldn't save the edit. \(sentence(e.description))")
        }
    }

    // MARK: direct messages

    /// Opens a DM with `user`: the existing 1:1 conversation if there is one (and shows it, loading its
    /// history), or asks the server to open one.
    public mutating func startDm(user: ID) {
        if let dm = oneToOneDm(with: user) {
            goHome()
            open(channel: dm.id)
        } else {
            outbox.append(.startDm(user: user))
        }
    }

    /// The reply to `startDm`: lists the conversation (idempotent: it may already be known, the gateway's own
    /// `dm_opened` or another device having opened it first) and shows it, loading its history — the same
    /// path a tap on an existing DM takes, so it never lands on the DM list instead of the conversation.
    public mutating func applyDmOpened(result: Result<DMChannel, AppError>) {
        switch result {
        case .success(let dm):
            addDm(dm)
            goHome()
            open(channel: dm.id)
        case .failure(let e):
            notice(.danger, "Couldn't open the conversation. \(sentence(e.description))")
        }
    }

    mutating func addDm(_ dm: DMChannel) {
        for u in dm.participants { users[u.id] = u }
        if !dms.contains(where: { $0.id == dm.id }) { dms.append(dm) }
    }

    /// The reply to a send.
    public mutating func applySent(channel: ID, nonce: String, result: Result<Message, AppError>) {
        switch result {
        case .success(let m):
            outgoing.removeAll { $0.nonce == nonce }
            if logs[channel]?.loaded == true { logs[channel]!.insert(m) }
        case .failure(let e):
            if let i = outgoing.firstIndex(where: { $0.nonce == nonce }) { outgoing[i].state = .failed }
            if !e.isTransient { notice(.danger, "Message not sent. \(sentence(e.description))") }
        }
    }

    // MARK: lists

    /// The first lists after sign-in (desktop's `initial`).
    public mutating func applySnapshot(_ s: Snapshot) {
        layouts = s.layouts.mapValues(\.normalized)
        me = s.me
        users[s.me.id] = s.me
        guilds = s.guilds
        channels = Dictionary(uniqueKeysWithValues: s.guilds.map { ($0.id, [Channel]()) })
        for c in s.channels { channels[c.guildID, default: []].append(c) }
        for dm in s.dms { for u in dm.participants { users[u.id] = u } }
        dms = s.dms
        phase = .signedIn
        place = .home
        openChannel = nil
    }

    /// The lists again after a gateway gap (desktop's `refreshed`). Unlike `applySnapshot`, the place, the
    /// selection and the loaded history stay as they are.
    public mutating func applyRefreshed(_ s: Snapshot) {
        guard phase == .signedIn else { return }
        for (g, l) in s.layouts where layoutUndo[g] == nil {
            // An edit still out is newer than this answer; its reply settles the layout.
            layouts[g] = l.normalized
        }
        // Roles and assignments may have changed while away: the open server asks again below, the others when visited.
        roles.removeAll(); memberRoles.removeAll(); accessAsked.removeAll()
        users[s.me.id] = s.me
        me = s.me
        for dm in s.dms { for u in dm.participants { users[u.id] = u } }
        channels = Dictionary(uniqueKeysWithValues: s.guilds.map { ($0.id, [Channel]()) })
        for c in s.channels { channels[c.guildID, default: []].append(c) }
        guilds = s.guilds
        dms = s.dms
        let open: ID? = if case .guild(let g) = place { g } else { nil }
        // Members who joined while away: the open server asks now, the others when visited.
        members = members.filter { $0.key == open }
        membersAsked = membersAsked.filter { $0 == open }
        emojis = emojis.filter { $0.key == open }
        emojisAsked = emojisAsked.filter { $0 == open }
        if let g = open {
            if guild(g) == nil {
                goHome()
            } else {
                outbox.append(.loadMembers(guild: g)); membersAsked.insert(g)
                outbox.append(.loadEmojis(guild: g)); emojisAsked.insert(g)
                wantAccess(g)
            }
        }
    }

    public mutating func applyChannels(guild: ID, channels list: [Channel], layout: Layout?) {
        channels[guild] = list
        if let layout, layoutUndo[guild] == nil { layouts[guild] = layout.normalized }
    }

    public mutating func applyMembers(guild: ID, members list: [Member]) {
        members[guild] = list.map(\.user.id)
        var assigned = memberRoles[guild] ?? [:]
        for m in list {
            users[m.user.id] = m.user
            assigned[m.user.id] = m.roleIDs
        }
        memberRoles[guild] = assigned
    }

    public mutating func applyRoles(guild: ID, roles list: [Role], version: UInt64?) {
        roles[guild] = list.sorted { $0.position > $1.position }
        if let version { roleVersions[guild] = version }
    }

    public mutating func applyEmojis(guild: ID, emojis list: [Emoji]) {
        emojis[guild] = list.sorted { $0.name.lowercased() < $1.name.lowercased() }
    }

    /// The reply to `ListMessages` (desktop's `messages`).
    public mutating func applyMessages(channel: ID, before: ID?, result: Result<MessagePage, AppError>) {
        guard var log = logs[channel] else { return }
        let page: MessagePage
        switch result {
        case .failure(let e):
            // A failed refresh of what is shown is tried again on the next visit.
            log.stale = before == nil && log.loaded
            if before != nil || log.fetch != .older { log.fetch = .idle }
            logs[channel] = log
            notice(.danger, "Couldn't load messages. \(sentence(e.description))")
            return
        case .success(let p):
            page = p
        }
        // A refresh of the newest page doesn't end an older page's fetch that is still out.
        if before != nil || log.fetch != .older { log.fetch = .idle }
        if before != nil {
            if let first = log.messages.first { log.breaks.append(first.id) }
            log.insertPage(page.messages)
            log.hasMore = page.hasMore
        } else {
            let newestPageOldest = page.messages.map(\.id).min()
            let gap = log.loaded && newestPageOldest != nil && log.newestID != nil && newestPageOldest! > log.newestID!
            if !log.loaded || gap {
                log.messages.removeAll()
                log.hasMore = page.hasMore
            }
            log.insertPage(page.messages)
            log.loaded = true
            log.stale = false
        }
        log.oldestID = log.messages.first?.id
        let strangers = log.messages.filter { users[$0.authorID] == nil }
        logs[channel] = log
        for m in strangers { wantAuthor(m) }
        resolveMarker(channel)
    }

    /// A "New" divider that was waiting for its history gets its place.
    mutating func resolveMarker(_ channel: ID) {
        guard case .pending(let c, let n) = marker, c == channel, let log = logs[channel] else { return }
        let at = log.messages.count - (n + 1)
        marker = .after(channel: channel, id: at >= 0 ? log.messages[at].id : nil)
    }

    // MARK: layout edits (optimistic, with rollback)

    /// Applies `new` at once and queues `UpdateLayout`. False when not allowed, or an edit is still out.
    @discardableResult
    public mutating func editLayout(guild: ID, _ new: Layout) -> Bool {
        guard canManageChannels(guild), layoutUndo[guild] == nil, let old = layouts[guild] else { return false }
        layouts[guild] = new
        layoutUndo[guild] = old
        outbox.append(.updateLayout(guild: guild, layout: new))
        return true
    }

    /// Takes the server's layout unless it is older than what is held.
    public mutating func setLayout(guild: ID, _ l: Layout) {
        if layouts[guild].map({ l.version >= $0.version }) ?? true { layouts[guild] = l.normalized }
    }

    /// The reply to `UpdateLayout`. Success: the server's layout. Failure: the edit comes back out (unless a newer
    /// layout already arrived on the gateway), and a stale version refetches.
    public mutating func applyLayoutReply(guild: ID, result: Result<Layout, AppError>) {
        let undo = layoutUndo.removeValue(forKey: guild)
        switch result {
        case .success(let l): setLayout(guild: guild, l)
        case .failure(let e):
            if let undo, layouts[guild]?.version == undo.version { layouts[guild] = undo }
            if e == .conflict {
                outbox.append(.loadChannels(guild: guild))
                notice(.info, "That was changed by someone else. Refreshed, try again.")
            } else {
                notice(.danger, "Couldn't change the layout. \(sentence(e.description))")
            }
        }
    }

    // MARK: reactions

    /// Your own reaction, optimistic. Returns whether the message changed.
    @discardableResult
    public mutating func setMyReaction(channel: ID, message: ID, emoji: ReactionEmoji, on: Bool) -> Bool {
        guard let m = logs[channel]?.messages.firstIndex(where: { $0.id == message }) else { return false }
        return Self.setMine(&logs[channel]!.messages[m], emoji, on)
    }

    static func setMine(_ m: inout Message, _ emoji: ReactionEmoji, _ mine: Bool) -> Bool {
        let at = m.reactions.firstIndex { $0.emoji == emoji }
        switch (at, mine) {
        case (let i?, true):
            if m.reactions[i].me { return false }
            m.reactions[i].me = true; m.reactions[i].count += 1
            return true
        case (nil, true):
            m.reactions.append(Reaction(emoji: emoji, count: 1, me: true))
            return true
        case (let i?, false):
            if !m.reactions[i].me { return false }
            m.reactions[i].me = false
            m.reactions[i].count = max(0, m.reactions[i].count - 1)
            if m.reactions[i].count == 0 { m.reactions.remove(at: i) }
            return true
        case (nil, false):
            return false
        }
    }

    static func setOther(_ m: inout Message, _ emoji: ReactionEmoji, _ add: Bool) {
        let at = m.reactions.firstIndex { $0.emoji == emoji }
        switch (at, add) {
        case (let i?, true): m.reactions[i].count += 1
        case (nil, true): m.reactions.append(Reaction(emoji: emoji, count: 1, me: false))
        case (let i?, false):
            m.reactions[i].count = max(0, m.reactions[i].count - 1)
            if m.reactions[i].count == 0 { m.reactions.remove(at: i) }
        case (nil, false): break
        }
    }

    /// A `reaction_added` / `reaction_removed` event. Your own come back too (the optimistic change already made
    /// them, so they change nothing); everyone else's move the count.
    mutating func reactionEvent(channel: ID, message: ID, emoji: ReactionEmoji, user: ID, add: Bool) {
        guard let i = logs[channel]?.messages.firstIndex(where: { $0.id == message }) else { return }
        if user == myID { _ = Self.setMine(&logs[channel]!.messages[i], emoji, add) }
        else { Self.setOther(&logs[channel]!.messages[i], emoji, add) }
    }

    // MARK: gateway

    public mutating func applyUpdate(_ update: GatewayUpdate) {
        switch update {
        case .connecting:
            if connection != .connected { connection = .connecting }
        case .connected:
            connection = .connected
            // A new stream starts with no voice states; a snapshot follows when anyone is in voice.
            voice.removeAll()
            // Events missed while disconnected aren't replayed: fetch them again.
            let was = stream
            stream = .live
            if was == .interrupted { resync() }
        case .disconnected(let retryIn, _):
            stream = .interrupted
            connection = .reconnecting(max(1, Int(retryIn.rounded())))
        case .authExpired:
            phase = .signedOut
        case .updateRequired:
            phase = .updateRequired
        case .event(let e):
            applyEvent(e)
        }
    }

    public mutating func applyEvent(_ e: GatewayEvent) {
        switch e {
        case .heartbeat: break
        case .emojisUpdated(let g, let list): applyEmojis(guild: g, emojis: list)
        case .reactionAdded(let c, let m, let emoji, let u): reactionEvent(channel: c, message: m, emoji: emoji, user: u, add: true)
        case .reactionRemoved(let c, let m, let emoji, let u): reactionEvent(channel: c, message: m, emoji: emoji, user: u, add: false)
        case .messageUpdated(let m): messageUpdated(m)
        case .guildUpdated(let g): if let i = guilds.firstIndex(where: { $0.id == g.id }) { guilds[i] = g }
        case .guildAdded(let g):
            // Joined or created elsewhere (another device): list it and load its channels and layout, leaving the place alone.
            if !guilds.contains(where: { $0.id == g.id }) { guilds.append(g) }
            if channels[g.id] == nil { outbox.append(.loadChannels(guild: g.id)) }
        case .channelUpdated(let c):
            if let i = channels[c.guildID]?.firstIndex(where: { $0.id == c.id }) { channels[c.guildID]![i] = c }
        case .channelCreated(let c):
            if !channels(of: c.guildID).contains(where: { $0.id == c.id }) { channels[c.guildID, default: []].append(c) }
        case .userUpdated(let u):
            users[u.id] = u
            if me?.id == u.id { me = u }
            for i in dms.indices { for j in dms[i].participants.indices where dms[i].participants[j].id == u.id { dms[i].participants[j] = u } }
        case .layoutUpdated(let g, let l): setLayout(guild: g, l)
        case .rolesUpdated(let g, let list, let v): applyRoles(guild: g, roles: list, version: v)
        case .memberUpdated(let g, let member):
            if memberRoles[g] != nil { memberRoles[g]![member.user.id] = member.roleIDs }
            users[member.user.id] = member.user
        case .voiceStateUpdate(let g, let u, let state): voiceUpdate(guild: g, user: u, state: state)
        case .voiceSnapshot(let gs, let states): voiceSnapshot(guilds: gs, states: states)
        case .voiceMoved(_, let c): voiceMovedTo = c
        case .messageDeleted(let c, let m): messageRemoved(channel: c, message: m)
        case .guildMemberRemoved(let g, let u, let reason): memberRemoved(guild: g, user: u, reason: reason)
        case .guildRemoved(let g): guildRemoved(g, reason: nil)
        case .channelDeleted(let g, let c): channelRemoved(guild: g, channel: c)
        case .notificationSettingsUpdated(let settings): notificationSettings = settings
        case .dmOpened(let dm): addDm(dm)
        case .guildMemberJoined(let g, let user):
            // Before the list is loaded the join is part of it; a list of one would pass for it.
            if members[g] != nil, !members[g]!.contains(user.id) { members[g]!.append(user.id) }
            users[user.id] = user
        case .messageCreated(let m): messageCreated(m)
        }
    }

    /// A message changed on the server (its embeds were resolved, an admin removed a file). Takes the new content,
    /// attachments and embeds; your reactions stay (events keep those current).
    mutating func messageUpdated(_ m: Message) {
        guard let i = logs[m.channelID]?.messages.firstIndex(where: { $0.id == m.id }) else { return }
        logs[m.channelID]!.messages[i].content = m.content
        logs[m.channelID]!.messages[i].editedAt = m.editedAt
        logs[m.channelID]!.messages[i].attachments = m.attachments
        logs[m.channelID]!.messages[i].embeds = m.embeds
    }

    /// Files an incoming message: reconciles a pending send by nonce, adds it to a loaded log, raises unread/mention counts.
    mutating func messageCreated(_ m: Message) {
        let channel = m.channelID
        if !m.nonce.isEmpty { outgoing.removeAll { $0.nonce == m.nonce } }
        wantAuthor(m)
        let visible = phase == .signedIn && openChannel == channel
        let mine = m.authorID == myID
        let mention = !mine && (dm(channel) != nil || mentionsMe(m.content))
        if logs[channel]?.loaded == true { logs[channel]!.insert(m) }
        if !visible && !mine {
            var u = unread[channel] ?? Unread()
            u.count += 1
            if mention { u.mentions += 1 }
            unread[channel] = u
        }
    }

    // MARK: moderation and structure events

    /// `message_deleted`: the message leaves its transcript.
    mutating func messageRemoved(channel: ID, message: ID) {
        guard logs[channel] != nil else { return }
        logs[channel]!.messages.removeAll { $0.id == message }
        logs[channel]!.breaks.removeAll { $0 == message }
        logs[channel]!.oldestID = logs[channel]!.messages.first?.id
    }

    /// `guild_member_removed`: the member leaves the lists. When it is us the guild goes too (`guild_removed`
    /// follows on the gateway; this just doesn't wait for it).
    mutating func memberRemoved(guild g: ID, user: ID, reason: MemberRemovalReason) {
        if user == myID { guildRemoved(g, reason: reason); return }
        members[g]?.removeAll { $0 == user }
        memberRoles[g]?.removeValue(forKey: user)
        voice[g]?.removeValue(forKey: user)
    }

    /// `guild_removed`: we are no longer in the guild. Said to the user unless they did it themselves.
    mutating func guildRemoved(_ g: ID, reason: MemberRemovalReason?) {
        guard let name = guild(g)?.name else { return }
        let own = exiting.contains(g)
        dropGuild(g)
        if own { return }
        switch reason {
        case .kicked?: notice(.info, "You were removed from \(name)")
        case .banned?: notice(.info, "You were banned from \(name)")
        case .left?: notice(.info, "You left \(name)")
        // The server says nothing more when a guild is deleted (left, kicked and banned come with a
        // `guild_member_removed` first).
        default: notice(.info, "\(name) was deleted")
        }
    }

    /// Forgets everything about a guild; if it was open the app goes home.
    public mutating func dropGuild(_ g: ID) {
        exiting.remove(g)
        let ids = channels(of: g).map(\.id)
        let wasOpen = place == .guild(g)
        voice.removeValue(forKey: g)
        for c in ids {
            logs.removeValue(forKey: c)
            unread.removeValue(forKey: c)
            outgoing.removeAll { $0.channel == c }
        }
        guilds.removeAll { $0.id == g }
        channels.removeValue(forKey: g)
        layouts.removeValue(forKey: g)
        layoutUndo.removeValue(forKey: g)
        roles.removeValue(forKey: g)
        roleVersions.removeValue(forKey: g)
        memberRoles.removeValue(forKey: g)
        accessAsked.remove(g)
        members.removeValue(forKey: g)
        membersAsked.remove(g)
        emojis.removeValue(forKey: g)
        emojisAsked.remove(g)
        collapsed = collapsed.filter { $0.guild != g }
        if wasOpen || openChannel.map(ids.contains) == true { goHome() }
    }

    /// `channel_deleted`: the channel leaves the lists; if it was open, its transcript closes.
    mutating func channelRemoved(guild g: ID, channel: ID) {
        channels[g]?.removeAll { $0.id == channel }
        for i in layouts[g]?.entries.indices ?? 0..<0 { layouts[g]!.entries[i].channelIDs.removeAll { $0 == channel } }
        logs.removeValue(forKey: channel)
        unread.removeValue(forKey: channel)
        outgoing.removeAll { $0.channel == channel }
        voice[g]?.keys.filter { voice[g]![$0]!.channelID == channel }.forEach { voice[g]!.removeValue(forKey: $0) }
        if openChannel == channel { closeChannel() }
    }

    // MARK: voice

    /// `voice_state_update`: nil state = the user left voice in `guild`.
    mutating func voiceUpdate(guild g: ID, user: ID, state: VoiceState?) {
        if state != nil {
            // A user is in one voice channel at a time, so a state in another guild is stale.
            for other in voice.keys where other != g { voice[other]?.removeValue(forKey: user) }
        }
        if let state { voice[g, default: [:]][user] = state } else { voice[g]?.removeValue(forKey: user) }
    }

    /// `voice_snapshot`: replaces the voice states of the listed guilds.
    mutating func voiceSnapshot(guilds gs: [ID], states: [VoiceState]) {
        var by: [ID: [ID: VoiceState]] = Dictionary(uniqueKeysWithValues: gs.map { ($0, [:]) })
        for s in states { by[s.guildID, default: [:]][s.userID] = s }
        for (g, users) in by { voice[g] = users }
    }
}
