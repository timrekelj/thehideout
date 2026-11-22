import Foundation
import Testing
@testable import HideoutCore

@Suite("Reducer")
struct ReducerTests {
    // MARK: snapshot, navigation

    @Test func snapshotSignsInAndNormalizesLayouts() async throws {
        let s = try await makeState()
        #expect(s.phase == .signedIn)
        #expect(s.me?.username == "tim")
        #expect(s.guilds.map(\.name) == ["The Hideout", "Garage Band", "Cooking Club"])
        #expect(s.channels(of: Ids.hideout).count == 12)
        #expect(s.layouts[Ids.hideout]?.entries.count == 5)
        #expect(s.dms.count == 2)
        #expect(s.users[Ids.ana]?.username == "ana")
        #expect(s.orderedChannels(Ids.hideout).first?.name == "general")
        #expect(s.firstTextChannel(Ids.hideout) == Ids.general)
    }

    @Test func enteringAGuildAsksOnceForMembersAccessAndEmojis() async throws {
        var s = try await makeState()
        s.enterGuild(Ids.hideout)
        #expect(s.place == .guild(Ids.hideout))
        #expect(s.outbox == [.loadMembers(guild: Ids.hideout), .loadAccess(guild: Ids.hideout), .loadEmojis(guild: Ids.hideout)])
        s.outbox.removeAll()
        s.enterGuild(Ids.hideout)
        #expect(s.outbox.isEmpty)
    }

    @Test func openingAChannelFetchesOnceAndClearsUnread() async throws {
        var s = try await makeState()
        s.unread[Ids.general] = Unread(count: 3, mentions: 1)
        s.open(channel: Ids.general)
        #expect(s.openChannel == Ids.general)
        #expect(s.unread[Ids.general] == nil)
        #expect(s.marker == .pending(channel: Ids.general, unread: 3))
        #expect(s.outbox == [.loadMessages(channel: Ids.general, before: nil)])
        s.open(channel: Ids.general)
        #expect(s.outbox.count == 1, "a fetch in flight is not asked again")
    }

    @Test func perms() async throws {
        var s = try await makeState()
        #expect(s.perms(Ids.hideout) == .all, "tim owns The Hideout")
        #expect(s.perms(Ids.garageBand) == .baseline, "roles unknown yet")
        s.applyRoles(guild: Ids.garageBand, roles: [Role(id: 2001, guildID: 2, name: "Roadie", permissions: .manageRoles)], version: 1)
        s.applyMembers(guild: Ids.garageBand, members: [Member(user: s.me!, roleIDs: [2001])])
        #expect(s.perms(Ids.garageBand) == [.connect, .speak, .video, .manageRoles])
        #expect(!s.canManageChannels(Ids.garageBand))
        #expect(s.canManageChannels(Ids.hideout))
    }

    // MARK: one test per gateway event

    @Test func heartbeatChangesNothing() async throws {
        var s = try await makeState(); let before = s.guilds
        s.applyEvent(.heartbeat)
        #expect(s.guilds == before && s.outbox.isEmpty)
    }

    @Test func messageCreatedInAClosedChannelRaisesUnreadAndMentions() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1, 2])
        s.closeChannel()
        s.applyEvent(.messageCreated(msg(3, by: Ids.bruno, "plain")))
        s.applyEvent(.messageCreated(msg(4, by: Ids.bruno, "hey @tim look")))
        s.applyEvent(.messageCreated(msg(5, by: Ids.bruno, "`@tim` in code does not count")))
        s.applyEvent(.messageCreated(msg(6, by: Ids.tim, "my own @tim")))
        #expect(s.unread(Ids.general) == Unread(count: 3, mentions: 1))
        #expect(s.log(Ids.general)?.messages.map(\.id) == [1, 2, 3, 4, 5, 6])
    }

    @Test func messageCreatedInTheOpenChannelIsNotUnread() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1])
        s.applyEvent(.messageCreated(msg(2, by: Ids.ana)))
        #expect(s.unread(Ids.general) == Unread())
        #expect(s.log(Ids.general)?.messages.count == 2)
    }

    @Test func aDMMessageAlwaysCountsAsAMention() async throws {
        var s = try await makeState()
        s.applyEvent(.messageCreated(msg(9, in: Ids.dmAna, by: Ids.ana, "ping", guild: nil)))
        #expect(s.unread(Ids.dmAna) == Unread(count: 1, mentions: 1))
        #expect(s.homeUnread == Unread(count: 1, mentions: 1))
    }

    @Test func messageCreatedFromAStrangerAsksForTheGuildMembers() async throws {
        var s = try await makeState()
        s.applyEvent(.messageCreated(msg(9, by: 999)))
        #expect(s.outbox == [.loadMembers(guild: Ids.hideout)])
    }

    @Test func channelCreatedAddsOnce() async throws {
        var s = try await makeState()
        let c = Channel(id: 77, guildID: Ids.hideout, name: "new")
        s.applyEvent(.channelCreated(c)); s.applyEvent(.channelCreated(c))
        #expect(s.channels(of: Ids.hideout).filter { $0.id == 77 }.count == 1)
        #expect(s.orderedChannels(Ids.hideout).last?.id == 77, "channels the layout doesn't list yet come last")
    }

    @Test func guildMemberJoinedJoinsLoadedListsOnly() async throws {
        var s = try await makeState()
        let u = User(id: 55, username: "newbie")
        s.applyEvent(.guildMemberJoined(guild: Ids.hideout, user: u))
        #expect(s.members[Ids.hideout] == nil && s.users[55] == u)
        s.applyMembers(guild: Ids.hideout, members: [Member(user: s.me!)])
        s.applyEvent(.guildMemberJoined(guild: Ids.hideout, user: u)); s.applyEvent(.guildMemberJoined(guild: Ids.hideout, user: u))
        #expect(s.members[Ids.hideout] == [Ids.tim, 55])
    }

    @Test func dmOpenedAddsTheDMAndItsUsers() async throws {
        var s = try await makeState()
        let dm = DMChannel(id: 60, participants: [s.me!, User(id: 8, username: "zed")])
        s.applyEvent(.dmOpened(dm)); s.applyEvent(.dmOpened(dm))
        #expect(s.dms.count == 3 && s.users[8]?.username == "zed")
    }

    @Test func guildUpdatedReplaces() async throws {
        var s = try await makeState()
        var g = s.guild(Ids.hideout)!; g.name = "Renamed"
        s.applyEvent(.guildUpdated(g))
        #expect(s.guild(Ids.hideout)?.name == "Renamed")
    }

    @Test func guildAddedListsItAndLoadsItsChannelsWithoutMovingThePlace() async throws {
        var s = try await makeState()
        s.enterGuild(Ids.garageBand); s.outbox.removeAll()
        s.applyEvent(.guildAdded(Guild(id: 9, name: "Elsewhere", ownerID: 1)))
        #expect(s.guild(9) != nil && s.place == .guild(Ids.garageBand))
        #expect(s.outbox == [.loadChannels(guild: 9)])
    }

    @Test func channelUpdatedReplaces() async throws {
        var s = try await makeState()
        var c = s.channel(Ids.dev, in: Ids.hideout)!; c.topic = "new topic"
        s.applyEvent(.channelUpdated(c))
        #expect(s.channel(Ids.dev, in: Ids.hideout)?.topic == "new topic")
    }

    @Test func layoutUpdatedTakesNewerVersionsOnly() async throws {
        var s = try await makeState()
        let v = s.layouts[Ids.hideout]!.version
        var newer = s.layouts[Ids.hideout]!; newer.version = v + 1; newer.entries[0].channelIDs = [Ids.dev, Ids.general, Ids.announcements]
        s.applyEvent(.layoutUpdated(guild: Ids.hideout, layout: newer))
        #expect(s.layouts[Ids.hideout]?.channelIDs(in: nil).first == Ids.dev)
        var older = newer; older.version = v - 1; older.entries[0].channelIDs = [Ids.announcements]
        s.applyEvent(.layoutUpdated(guild: Ids.hideout, layout: older))
        #expect(s.layouts[Ids.hideout]?.version == v + 1)
    }

    @Test func rolesUpdatedSortsAndKeepsTheVersion() async throws {
        var s = try await makeState()
        s.applyEvent(.rolesUpdated(guild: 2, roles: [Role(id: 1, guildID: 2, name: "a", position: 1), Role(id: 2, guildID: 2, name: "b", position: 5)], version: 9))
        #expect(s.roles[2]?.map(\.id) == [2, 1] && s.roleVersions[2] == 9)
    }

    @Test func memberUpdatedChangesRolesOfLoadedGuildsAndStoresTheUser() async throws {
        var s = try await makeState()
        s.applyMembers(guild: Ids.hideout, members: [Member(user: s.me!, roleIDs: [])])
        s.applyEvent(.memberUpdated(guild: Ids.hideout, member: Member(user: User(id: 2, username: "ana2"), roleIDs: [1003])))
        #expect(s.memberRoles[Ids.hideout]?[2] == [1003] && s.users[2]?.username == "ana2")
        s.applyEvent(.memberUpdated(guild: Ids.garageBand, member: Member(user: User(id: 3, username: "b"), roleIDs: [1])))
        #expect(s.memberRoles[Ids.garageBand] == nil, "an unloaded guild's roles come with its member list")
    }

    @Test func userUpdatedReachesMeDMsAndTheUserMap() async throws {
        var s = try await makeState()
        var u = s.users[Ids.ana]!; u.avatar = "media/abc"
        s.applyEvent(.userUpdated(u))
        #expect(s.users[Ids.ana]?.avatar == "media/abc")
        #expect(s.dm(Ids.dmAna)?.participants.first { $0.id == Ids.ana }?.avatar == "media/abc")
        var me = s.me!; me.avatar = "media/me"
        s.applyEvent(.userUpdated(me))
        #expect(s.me?.avatar == "media/me")
    }

    @Test func emojisUpdatedSortsByName() async throws {
        var s = try await makeState()
        s.applyEvent(.emojisUpdated(guild: 1, emojis: [Emoji(id: 2, guildID: 1, name: "wobble"), Emoji(id: 1, guildID: 1, name: "blob")]))
        #expect(s.emojis[1]?.map(\.name) == ["blob", "wobble"])
    }

    @Test func reactionsFromOthersMoveTheCountAndOwnComeBackIdempotent() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1])
        let thumbs = ReactionEmoji.unicode("👍")
        s.applyEvent(.reactionAdded(channel: Ids.general, message: 1, emoji: thumbs, user: Ids.ana))
        s.applyEvent(.reactionAdded(channel: Ids.general, message: 1, emoji: thumbs, user: Ids.bruno))
        #expect(s.log(Ids.general)?.messages[0].reactions == [Reaction(emoji: thumbs, count: 2, me: false)])
        // Optimistic own reaction, then the event echo changes nothing.
        s.setMyReaction(channel: Ids.general, message: 1, emoji: thumbs, on: true)
        s.applyEvent(.reactionAdded(channel: Ids.general, message: 1, emoji: thumbs, user: Ids.tim))
        #expect(s.log(Ids.general)?.messages[0].reactions == [Reaction(emoji: thumbs, count: 3, me: true)])
        s.applyEvent(.reactionRemoved(channel: Ids.general, message: 1, emoji: thumbs, user: Ids.ana))
        s.applyEvent(.reactionRemoved(channel: Ids.general, message: 1, emoji: thumbs, user: Ids.bruno))
        s.applyEvent(.reactionRemoved(channel: Ids.general, message: 1, emoji: thumbs, user: Ids.tim))
        #expect(s.log(Ids.general)?.messages[0].reactions.isEmpty == true, "a reaction at zero disappears")
    }

    @Test func messageUpdatedTakesContentAndEmbedsButKeepsReactions() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1])
        s.setMyReaction(channel: Ids.general, message: 1, emoji: .unicode("🎉"), on: true)
        var updated = msg(1, "edited"); updated.editedAt = Date(timeIntervalSince1970: 99)
        updated.embeds = [Embed(url: "https://example.com", title: "Example")]
        s.applyEvent(.messageUpdated(updated))
        let m = s.log(Ids.general)!.messages[0]
        #expect(m.content == "edited" && m.editedAt != nil && m.embeds.count == 1 && m.reactions.count == 1)
    }

    @Test func voiceStateUpdateAddsMovesAndRemoves() async throws {
        var s = try await makeState()
        let st = VoiceState(guildID: 1, channelID: Ids.lounge, userID: Ids.ana)
        s.applyEvent(.voiceStateUpdate(guild: 1, user: Ids.ana, state: st))
        #expect(s.participants(of: Ids.lounge, in: 1).map(\.userID) == [Ids.ana])
        // The same user in another guild's voice removes the stale one.
        s.applyEvent(.voiceStateUpdate(guild: 2, user: Ids.ana, state: VoiceState(guildID: 2, channelID: 99, userID: Ids.ana)))
        #expect(s.voice[1]?[Ids.ana] == nil && s.voice[2]?[Ids.ana] != nil)
        s.applyEvent(.voiceStateUpdate(guild: 2, user: Ids.ana, state: nil))
        #expect(s.voice[2]?[Ids.ana] == nil)
    }

    @Test func voiceSnapshotReplacesTheListedGuildsOnly() async throws {
        var s = try await makeState()
        s.voice[3] = [Ids.tim: VoiceState(guildID: 3, channelID: 31, userID: Ids.tim)]
        s.voice[1] = [Ids.cvetka: VoiceState(guildID: 1, channelID: Ids.lounge, userID: Ids.cvetka)]
        s.applyEvent(.voiceSnapshot(guilds: [1, 2], states: [VoiceState(guildID: 1, channelID: Ids.lounge, userID: Ids.ana, selfMute: true)]))
        #expect(s.voice[1]?.keys.sorted() == [Ids.ana] && s.voice[2]?.isEmpty == true && s.voice[3]?[Ids.tim] != nil)
        #expect(s.voiceState(of: Ids.ana)?.selfMute == true)
    }

    @Test func voiceMovedRemembersTheChannel() async throws {
        var s = try await makeState()
        s.applyEvent(.voiceMoved(guild: 1, channel: Ids.gameNight))
        #expect(s.voiceMovedTo == Ids.gameNight)
    }

    @Test func messageDeletedLeavesTheLog() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1, 2, 3])
        s.applyEvent(.messageDeleted(channel: Ids.general, message: 1))
        #expect(s.log(Ids.general)?.messages.map(\.id) == [2, 3] && s.log(Ids.general)?.oldestID == 2)
    }

    @Test func memberRemovedDropsTheMemberFromListsAndVoice() async throws {
        var s = try await makeState()
        s.applyMembers(guild: 1, members: [Member(user: s.me!), Member(user: s.users[Ids.ana]!, roleIDs: [1003])])
        s.voice[1] = [Ids.ana: VoiceState(guildID: 1, channelID: Ids.lounge, userID: Ids.ana)]
        s.applyEvent(.guildMemberRemoved(guild: 1, user: Ids.ana, reason: .kicked))
        #expect(s.members[1] == [Ids.tim] && s.memberRoles[1]?[Ids.ana] == nil && s.voice[1]?[Ids.ana] == nil)
        #expect(s.notices.isEmpty, "somebody else's removal is not news")
    }

    @Test func beingRemovedDropsTheGuildAndSaysWhy() async throws {
        var s = try await makeState()
        s.enterGuild(Ids.garageBand)
        s.applyEvent(.guildMemberRemoved(guild: 2, user: Ids.tim, reason: .banned))
        #expect(s.guild(2) == nil && s.place == .home && s.notices.map(\.text) == ["You were banned from Garage Band"])
        s.applyEvent(.guildRemoved(guild: 2))  // the follow-up event is a no-op
        #expect(s.notices.count == 1)
    }

    @Test func guildRemovedForADeletedGuildForgetsEverything() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1])
        s.applyEvent(.guildRemoved(guild: 1))
        #expect(s.guild(1) == nil && s.channels[1] == nil && s.layouts[1] == nil && s.logs[Ids.general] == nil)
        #expect(s.openChannel == nil && s.notices.map(\.text) == ["The Hideout was deleted"])
    }

    @Test func leavingYourselfIsNotAnnounced() async throws {
        var s = try await makeState()
        s.exiting.insert(2)
        s.applyEvent(.guildRemoved(guild: 2))
        #expect(s.guild(2) == nil && s.notices.isEmpty && s.exiting.isEmpty)
    }

    @Test func channelDeletedClosesAnOpenTranscriptAndCleansUp() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1])
        s.voice[1] = [Ids.ana: VoiceState(guildID: 1, channelID: Ids.lounge, userID: Ids.ana)]
        s.applyEvent(.channelDeleted(guild: 1, channel: Ids.general))
        #expect(s.channel(Ids.general, in: 1) == nil && s.logs[Ids.general] == nil && s.openChannel == nil)
        #expect(!(s.layouts[1]?.channelOrder.contains(Ids.general) ?? true))
        s.applyEvent(.channelDeleted(guild: 1, channel: Ids.lounge))
        #expect(s.voice[1]?[Ids.ana] == nil)
    }

    @Test func notificationSettingsUpdatedStoresTheFullSettings() async throws {
        var s = try await makeState()
        #expect(s.notificationSettings == nil)
        let first = NotificationSettings(dmsEnabled: false, guildDefault: .all, showPreviews: false,
                                         guilds: [NotificationOverride(id: Ids.hideout, level: .nothing)],
                                         channels: [NotificationOverride(id: Ids.dev, level: .mentions, mutedUntil: Date(timeIntervalSince1970: 4_000_000_000))])
        s.applyEvent(.notificationSettingsUpdated(first))
        #expect(s.notificationSettings == first)
        // The event carries the whole settings, so a second one replaces, it does not merge.
        let second = NotificationSettings(guilds: [NotificationOverride(id: Ids.garageBand, level: .all)])
        s.applyEvent(.notificationSettingsUpdated(second))
        #expect(s.notificationSettings == second && s.notificationSettings?.channels.isEmpty == true)
        #expect(s.outbox.isEmpty, "no refetch: the event is the new state")
    }

    // MARK: connection and the Interrupted refetch

    @Test func disconnectedMarksTheStreamInterruptedAndConnectedRefetches() async throws {
        var s = try await makeState()
        loaded(&s, ids: [1, 2])
        s.logs[Ids.dev] = { var l = ChannelLog(); l.loaded = true; return l }()
        s.applyUpdate(.connecting)
        s.applyUpdate(.connected)
        #expect(s.connection == .connected && s.outbox.isEmpty, "a first connect has nothing to refetch")
        s.applyUpdate(.disconnected(retryIn: 2.4, reason: "x"))
        #expect(s.stream == .interrupted && s.connection == .reconnecting(2))
        s.applyUpdate(.connecting)
        #expect(s.connection == .connecting && s.stream == .interrupted, "connecting overwrites reconnecting, which is why `stream` exists")
        s.voice[1] = [Ids.ana: VoiceState(guildID: 1, channelID: Ids.lounge, userID: Ids.ana)]
        s.applyUpdate(.connected)
        #expect(s.stream == .live && s.connection == .connected && s.voice.isEmpty)
        #expect(s.outbox == [.refresh, .loadMessages(channel: Ids.general, before: nil)])
        #expect(s.logs[Ids.dev]?.stale == true && s.logs[Ids.general]?.stale == false, "other channels refetch on their next visit")
        s.outbox.removeAll()
        s.open(channel: Ids.dev)
        #expect(s.outbox == [.loadMessages(channel: Ids.dev, before: nil)] && s.logs[Ids.dev]?.stale == false)
    }

    @Test func authExpiredSignsOutAndUpdateRequiredBlocks() async throws {
        var s = try await makeState()
        s.applyUpdate(.updateRequired)
        #expect(s.phase == .updateRequired)
        s.applyUpdate(.authExpired)
        #expect(s.phase == .signedOut)
    }

    @Test func refreshedKeepsThePlaceAndHistoryButReplacesLists() async throws {
        var s = try await makeState()
        s.enterGuild(Ids.hideout); s.outbox.removeAll()
        loaded(&s, ids: [1, 2])
        var snap = try await FakeBackend().snapshot()
        snap.guilds.removeAll { $0.id == 3 }
        snap.channels.append(Channel(id: 88, guildID: 1, name: "fresh"))
        s.applyRefreshed(snap)
        #expect(s.guild(3) == nil && s.channel(88, in: 1) != nil)
        #expect(s.place == .guild(Ids.hideout) && s.openChannel == Ids.general && s.log(Ids.general)?.messages.count == 2)
        #expect(s.outbox.contains(.loadMembers(guild: 1)) && s.outbox.contains(.loadEmojis(guild: 1)) && s.outbox.contains(.loadAccess(guild: 1)))
        // The open server vanishing sends us home.
        snap.guilds.removeAll { $0.id == 1 }
        s.applyRefreshed(snap)
        #expect(s.place == .home && s.openChannel == nil)
    }

    @Test func refreshedKeepsAnOptimisticLayoutEditThatIsStillOut() async throws {
        var s = try await makeState()
        var edit = s.layouts[1]!; edit.entries[0].channelIDs.reverse()
        let edited = s.editLayout(guild: 1, edit)
        #expect(edited)
        s.applyRefreshed(try await FakeBackend().snapshot())
        #expect(s.layouts[1]?.channelIDs(in: nil) == edit.channelIDs(in: nil))
    }
}
