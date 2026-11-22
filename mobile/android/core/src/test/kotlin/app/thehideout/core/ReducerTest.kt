package app.thehideout.core

import app.thehideout.core.api.AppError
import app.thehideout.core.api.GuildMembers
import app.thehideout.core.api.GuildRoles
import app.thehideout.core.api.MessagePage
import app.thehideout.core.fake.FakeBackend.Companion.ANA
import app.thehideout.core.fake.FakeBackend.Companion.BRUNO
import app.thehideout.core.fake.FakeBackend.Companion.CVETKA
import app.thehideout.core.fake.FakeBackend.Companion.DEV
import app.thehideout.core.fake.FakeBackend.Companion.DM_ANA
import app.thehideout.core.fake.FakeBackend.Companion.GARAGE
import app.thehideout.core.fake.FakeBackend.Companion.GENERAL
import app.thehideout.core.fake.FakeBackend.Companion.HIDEOUT
import app.thehideout.core.fake.FakeBackend.Companion.LOUNGE
import app.thehideout.core.fake.FakeBackend.Companion.GAME_NIGHT
import app.thehideout.core.fake.FakeBackend.Companion.TIM
import app.thehideout.core.model.*
import app.thehideout.core.store.*
import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test
import java.time.Duration

class ReducerTest {
    private val s0 = loadedState()

    // ---- the first load ----

    @Test fun `the initial load signs in and opens the first server on its first text channel`() {
        assertEquals(SessionPhase.SignedIn, s0.phase)
        assertEquals(Place.InGuild(HIDEOUT), s0.place)
        assertEquals(GENERAL, s0.currentChannel)
        assertEquals(listOf("The Hideout", "Garage Band", "Cooking Club"), s0.guilds.map { it.name })
        assertEquals(setOf(TIM, ANA, BRUNO), s0.users.keys - CVETKA)
    }

    @Test fun `opening a guild fetches its first page, its members and roles once`() {
        val r = Reducer.reduce(AppState(), Action.InitialLoaded(kotlinx.coroutines.runBlocking { app.thehideout.core.fake.FakeBackend().loadInitial() }))
        assertEquals(
            listOf(Effect.LoadMembers(HIDEOUT), Effect.LoadAccess(HIDEOUT), Effect.LoadMessages(GENERAL, null)),
            r.effects,
        )
        val again = Reducer.reduce(r.state, Action.OpenGuild(GARAGE)).state
        val back = Reducer.reduce(again, Action.OpenGuild(HIDEOUT))
        assertEquals(emptyList<Effect>(), back.effects, "already asked for members, roles and the first page")
    }

    @Test fun `sidebar order follows the layout and voice channels are not opened`() {
        assertEquals(
            listOf("general", "dev", "announcements", "design", "plans", "random", "music", "gaming", "memes", "books", "lounge", "game-night"),
            s0.orderedChannels(HIDEOUT).map { it.name },
        )
        val voice = s0.reduce(Action.OpenChannel(LOUNGE))
        assertEquals(GENERAL, voice.currentChannel)
    }

    // ---- gateway events, one per kind ----

    @Test fun `heartbeat changes nothing`() = assertEquals(s0, s0.event(GatewayEvent.Heartbeat))

    @Test fun `message_created in the open channel lands in the log and is not unread`() {
        val s = s0.withLog(GENERAL, listOf(1, 2)).event(GatewayEvent.MessageCreated(msg(3, GENERAL, ANA)))
        assertEquals(listOf(1L, 2L, 3L), s.log(GENERAL)!!.messages.map { it.id })
        assertEquals(Unread(), s.unreadOf(GENERAL))
    }

    @Test fun `message_created elsewhere counts unread and mentions`() {
        val s = s0.event(GatewayEvent.MessageCreated(msg(5, DEV, BRUNO, "hello @tim")))
            .event(GatewayEvent.MessageCreated(msg(6, DEV, ANA, "plain")))
        assertEquals(Unread(2, 1), s.unreadOf(DEV))
        assertEquals(Unread(2, 1), s.guildUnread(HIDEOUT))
        assertEquals(Unread(), s.unreadOf(GENERAL))
    }

    @Test fun `a message in a DM always counts as a mention, one of mine never counts`() {
        val dm = s0.event(GatewayEvent.MessageCreated(msg(7, DM_ANA, ANA, "hi", guild = null)))
        assertEquals(Unread(1, 1), dm.unreadOf(DM_ANA))
        assertEquals(Unread(1, 1), dm.homeUnread)
        val mine = s0.event(GatewayEvent.MessageCreated(msg(8, DEV, TIM, "from my other device")))
        assertEquals(Unread(), mine.unreadOf(DEV))
    }

    @Test fun `visiting a channel clears its unread count and places the New divider`() {
        val s = s0.withLog(DEV, listOf(1, 2, 3)).event(GatewayEvent.MessageCreated(msg(4, DEV, ANA))).event(GatewayEvent.MessageCreated(msg(5, DEV, ANA)))
        val visited = s.reduce(Action.OpenChannel(DEV))
        assertEquals(Unread(), visited.unreadOf(DEV))
        assertEquals(Marker.After(DEV, 3L), visited.marker, "divider after the last read message")
    }

    @Test fun `message_updated takes content, embeds and attachments but keeps reactions`() {
        val reacted = msg(1, GENERAL, ANA).copy(reactions = listOf(Reaction(ReactionEmoji.Unicode("🎉"), 2, true)))
        val s = s0.copy(logs = s0.logs + (GENERAL to ChannelLog(messages = listOf(reacted), loaded = true)))
        val embed = Embed(url = "https://example.com", title = "Example")
        val updated = s.event(GatewayEvent.MessageUpdated(msg(1, GENERAL, ANA, "edited").copy(embeds = listOf(embed), editedAt = T0)))
        val m = updated.log(GENERAL)!!.messages.single()
        assertEquals("edited", m.content)
        assertEquals(listOf(embed), m.embeds)
        assertEquals(T0, m.editedAt)
        assertEquals(1, m.reactions.size)
    }

    @Test fun `message_deleted removes the message and moves the oldest id`() {
        val s = s0.withLog(GENERAL, listOf(1, 2, 3)).event(GatewayEvent.MessageDeleted(GENERAL, 1))
        assertEquals(listOf(2L, 3L), s.log(GENERAL)!!.messages.map { it.id })
        assertEquals(2L, s.log(GENERAL)!!.oldestId)
        assertEquals(s, s.event(GatewayEvent.MessageDeleted(999, 1)), "an unknown channel is ignored")
    }

    @Test fun `reactions from others move the count, mine only flip my flag`() {
        val s = s0.withLog(GENERAL, listOf(1))
        val up = Unicode("👍")
        val other = s.event(GatewayEvent.ReactionAdded(GENERAL, 1, up, ANA))
        assertEquals(Reaction(up, 1, false), other.log(GENERAL)!!.messages.single().reactions.single())
        val twice = other.event(GatewayEvent.ReactionAdded(GENERAL, 1, up, BRUNO))
        assertEquals(2, twice.log(GENERAL)!!.messages.single().reactions.single().count)
        val mine = twice.event(GatewayEvent.ReactionAdded(GENERAL, 1, up, TIM))
        assertEquals(Reaction(up, 3, true), mine.log(GENERAL)!!.messages.single().reactions.single())
        val echo = mine.event(GatewayEvent.ReactionAdded(GENERAL, 1, up, TIM))
        assertEquals(mine, echo, "my own echo changes nothing")
        val gone = echo.event(GatewayEvent.ReactionRemoved(GENERAL, 1, up, TIM)).event(GatewayEvent.ReactionRemoved(GENERAL, 1, up, ANA)).event(GatewayEvent.ReactionRemoved(GENERAL, 1, up, BRUNO))
        assertTrue(gone.log(GENERAL)!!.messages.single().reactions.isEmpty(), "the last removal drops the reaction")
    }

    private fun Unicode(e: String) = ReactionEmoji.Unicode(e)

    @Test fun `toggling my reaction is optimistic and asks the server`() {
        val s = s0.withLog(GENERAL, listOf(1))
        val up = Unicode("👍")
        val on = Reducer.reduce(s, Action.React(GENERAL, 1, up))
        assertEquals(listOf(Effect.AddReaction(GENERAL, 1, up)), on.effects)
        assertEquals(Reaction(up, 1, true), on.state.log(GENERAL)!!.messages.single().reactions.single())
        val off = Reducer.reduce(on.state, Action.React(GENERAL, 1, up))
        assertEquals(listOf(Effect.RemoveReaction(GENERAL, 1, up)), off.effects)
        assertTrue(off.state.log(GENERAL)!!.messages.single().reactions.isEmpty())
    }

    // ---- edit, delete, drafts, mentions ----

    @Test fun `editing a message updates it locally and asks the server`() {
        val s = s0.withLog(GENERAL, listOf(1), author = TIM)
        val r = Reducer.reduce(s, Action.EditMessage(GENERAL, 1, "  new text  "))
        assertEquals(listOf(Effect.EditMessage(GENERAL, 1, "new text", previous = "m1")), r.effects)
        assertEquals("new text", r.state.log(GENERAL)!!.messages.single().content)
    }

    @Test fun `an empty or too-long edit is dropped`() {
        val s = s0.withLog(GENERAL, listOf(1), author = TIM)
        assertEquals(Reduction(s), Reducer.reduce(s, Action.EditMessage(GENERAL, 1, "   ")))
        val tooLong = "x".repeat(MAX_MESSAGE_CHARS + 1)
        assertEquals(Reduction(s), Reducer.reduce(s, Action.EditMessage(GENERAL, 1, tooLong)))
    }

    @Test fun `a failed edit rolls back to what was there and raises a notice`() {
        val s = s0.withLog(GENERAL, listOf(1), author = TIM).reduce(Action.EditMessage(GENERAL, 1, "edited"))
        assertEquals("edited", s.log(GENERAL)!!.messages.single().content, "optimistic while the server is asked")
        val failed = s.reduce(Action.MessageEdited(EditOutcome.Failed(GENERAL, 1, "m1", AppError.PermissionDenied)))
        assertEquals("m1", failed.log(GENERAL)!!.messages.single().content, "rolled back to what was there before the edit")
        assertEquals(1, failed.notices.size)
    }

    @Test fun `deleting asks the server, message_deleted then removes it from the transcript`() {
        val s = s0.withLog(GENERAL, listOf(1, 2), author = TIM)
        val r = Reducer.reduce(s, Action.DeleteMessage(GENERAL, 1))
        assertEquals(listOf(Effect.DeleteMessage(GENERAL, 1)), r.effects)
        assertEquals(listOf(1L, 2L), r.state.log(GENERAL)!!.messages.map { it.id }, "removal waits for the event")
        val gone = r.state.event(GatewayEvent.MessageDeleted(GENERAL, 1))
        assertEquals(listOf(2L), gone.log(GENERAL)!!.messages.map { it.id })
    }

    @Test fun `a draft is kept per channel and an empty one clears it`() {
        val s = s0.reduce(Action.SetDraft(GENERAL, "hello")).reduce(Action.SetDraft(DEV, "world"))
        assertEquals("hello", s.draft(GENERAL))
        assertEquals("world", s.draft(DEV))
        val cleared = s.reduce(Action.SetDraft(GENERAL, ""))
        assertEquals("", cleared.draft(GENERAL))
        assertFalse(cleared.drafts.containsKey(GENERAL))
    }

    @Test fun `the mention feed lists loaded messages that mention me, newest first`() {
        val log = ChannelLog(
            messages = listOf(
                msg(1, DEV, ANA, "hi @tim"),
                msg(2, DEV, BRUNO, "no mention here"),
                msg(3, DEV, ANA, "@tim again, later"),
            ),
            loaded = true,
        )
        val s = s0.copy(logs = s0.logs + (DEV to log))
        val feed = s.mentionFeed()
        assertEquals(listOf(3L, 1L), feed.map { it.message.id })
        assertTrue(feed.all { it.channelId == DEV })
    }

    @Test fun `starting a dm with someone already open switches to it and fetches its history`() {
        val r = Reducer.reduce(s0, Action.StartDm(ANA))
        assertEquals(listOf(Effect.LoadMessages(DM_ANA, null)), r.effects, "DM_ANA exists but its log is not loaded yet: arrive() must still fetch it")
        assertEquals(Place.Home, r.state.place)
        assertEquals(DM_ANA, r.state.homeDm)
    }

    @Test fun `starting a dm whose history is already loaded switches with no further effect`() {
        val s = s0.withLog(DM_ANA, listOf(40, 41))
        val r = Reducer.reduce(s, Action.StartDm(ANA))
        assertEquals(emptyList<Effect>(), r.effects)
        assertEquals(DM_ANA, r.state.homeDm)
    }

    @Test fun `starting a dm never matches myself or a dm that is not 1 to 1`() {
        assertEquals(listOf(Effect.OpenDm(TIM)), Reducer.reduce(s0, Action.StartDm(TIM)).effects, "never matches myself, even if listed somehow")
        // CVETKA has no 1:1 DM in the fake seed, so a 3-participant channel that happens to list her must not count as one.
        val group = DmChannel(90, listOf(s0.me!!, s0.users.getValue(ANA), User(CVETKA, "cvetka", T0)), T0)
        val withGroup = s0.copy(dms = s0.dms + group)
        assertEquals(listOf(Effect.OpenDm(CVETKA)), Reducer.reduce(withGroup, Action.StartDm(CVETKA)).effects, "a 3-participant channel is not a 1:1 match")
    }

    @Test fun `starting a dm with someone new asks the server, then opens it and fetches its history`() {
        val r = Reducer.reduce(s0, Action.StartDm(999))
        assertEquals(listOf(Effect.OpenDm(999)), r.effects)
        val dm = DmChannel(70, listOf(s0.me!!, User(999, "newperson", T0)), T0)
        val opened = Reducer.reduce(r.state, Action.DmOpened(Result.success(dm)))
        assertEquals(listOf(Effect.LoadMessages(70, null)), opened.effects, "a freshly opened DM has no history yet: must be fetched")
        val finalState = opened.state
        assertEquals(Place.Home, finalState.place)
        assertEquals(70L, finalState.homeDm)
        assertTrue(finalState.dms.any { it.id == 70L })
    }

    @Test fun `the mention feed never includes my own messages`() {
        val s = s0.withLog(DEV, listOf(1)).copy(
            logs = s0.logs + (DEV to ChannelLog(messages = listOf(msg(1, DEV, TIM, "@tim talking to myself")), loaded = true)),
        )
        assertTrue(s.mentionFeed().isEmpty())
    }

    @Test fun `channel_created adds once, channel_updated replaces`() {
        val c = Channel(77, HIDEOUT, "new", T0)
        val added = s0.event(GatewayEvent.ChannelCreated(c)).event(GatewayEvent.ChannelCreated(c))
        assertEquals(1, added.channelsOf(HIDEOUT).count { it.id == 77L })
        val renamed = added.event(GatewayEvent.ChannelUpdated(c.copy(name = "renamed", topic = "t")))
        assertEquals("renamed", renamed.channel(HIDEOUT, 77)!!.name)
        assertEquals("t", renamed.channel(HIDEOUT, 77)!!.topic)
    }

    @Test fun `channel_deleted drops the channel, its log and unread, and reopens the first text channel`() {
        val s = s0.withLog(GENERAL, listOf(1)).event(GatewayEvent.ChannelDeleted(HIDEOUT, GENERAL))
        assertNull(s.channel(HIDEOUT, GENERAL))
        assertNull(s.log(GENERAL))
        assertFalse(s.layouts.getValue(HIDEOUT).entries.any { GENERAL in it.channelIds })
        assertEquals(DEV, s.currentChannel, "the next text channel in sidebar order opens")
    }

    @Test fun `channel_deleted removes voice states that were in it`() {
        val joined = VoiceState(HIDEOUT, LOUNGE, ANA, joinedAt = T0)
        val s = s0.event(GatewayEvent.VoiceStateUpdate(HIDEOUT, ANA, joined)).event(GatewayEvent.ChannelDeleted(HIDEOUT, LOUNGE))
        assertTrue(s.voice[HIDEOUT].orEmpty().isEmpty())
    }

    @Test fun `member_joined adds to a loaded list only`() {
        val u = User(99, "newbie", T0)
        val unloaded = s0.event(GatewayEvent.MemberJoined(HIDEOUT, u))
        assertNull(unloaded.members[HIDEOUT], "before the list is loaded the join is part of it")
        assertEquals(u, unloaded.users[99])
        val loaded = unloaded.reduce(Action.MembersLoaded(HIDEOUT, Result.success(GuildMembers(emptyList(), listOf(Member(u.copy(id = TIM), emptyList(), T0))))))
        assertEquals(listOf(TIM), loaded.members[HIDEOUT])
        assertEquals(listOf(TIM, 99L), loaded.event(GatewayEvent.MemberJoined(HIDEOUT, u)).members[HIDEOUT])
    }

    @Test fun `member_updated stores roles once access is loaded and always the user`() {
        val u = User(ANA, "ana2", T0)
        val s = s0.event(GatewayEvent.MemberUpdated(HIDEOUT, Member(u, listOf(1003), T0)))
        assertEquals("ana2", s.users[ANA]!!.username)
        assertNull(s.memberRoles[HIDEOUT])
        val loaded = s.reduce(Action.MembersLoaded(HIDEOUT, Result.success(GuildMembers(emptyList(), listOf(Member(u, emptyList(), T0))))))
        assertEquals(listOf(1003L), loaded.event(GatewayEvent.MemberUpdated(HIDEOUT, Member(u, listOf(1003), T0))).memberRoles[HIDEOUT]!![ANA])
    }

    @Test fun `guild_member_removed of someone else shrinks the lists and the voice state`() {
        val loaded = s0.reduce(Action.MembersLoaded(HIDEOUT, Result.success(GuildMembers(emptyList(), listOf(TIM, ANA, BRUNO).map { Member(User(it, "u$it", T0), emptyList(), T0) }))))
            .event(GatewayEvent.VoiceStateUpdate(HIDEOUT, BRUNO, VoiceState(HIDEOUT, LOUNGE, BRUNO, joinedAt = T0)))
        val s = loaded.event(GatewayEvent.MemberRemoved(HIDEOUT, BRUNO, RemovalReason.Kicked))
        assertEquals(listOf(TIM, ANA), s.members[HIDEOUT])
        assertNull(s.memberRoles[HIDEOUT]!![BRUNO])
        assertNull(s.voice[HIDEOUT]!![BRUNO])
        assertEquals(HIDEOUT, (s.place as Place.InGuild).id)
    }

    @Test fun `guild_member_removed of me is a kick or a ban and drops the guild`() {
        val kicked = s0.event(GatewayEvent.MemberRemoved(HIDEOUT, TIM, RemovalReason.Kicked))
        assertNull(kicked.guild(HIDEOUT))
        assertEquals(Place.Home, kicked.place)
        assertEquals("You were removed from The Hideout", kicked.notices.last().text)
        val banned = s0.event(GatewayEvent.MemberRemoved(HIDEOUT, TIM, RemovalReason.Banned))
        assertEquals("You were banned from The Hideout", banned.notices.last().text)
    }

    @Test fun `guild_removed forgets the guild wholesale and goes home if it was open`() {
        val s = s0.withLog(GENERAL, listOf(1)).event(GatewayEvent.GuildRemoved(HIDEOUT))
        assertNull(s.guild(HIDEOUT))
        assertNull(s.channels[HIDEOUT]); assertNull(s.layouts[HIDEOUT]); assertNull(s.log(GENERAL))
        assertEquals(Place.Home, s.place)
        assertEquals("The Hideout was deleted", s.notices.last().text)
    }

    @Test fun `guild_removed after our own leave is not announced`() {
        val s = s0.reduce(Action.Exiting(GARAGE)).event(GatewayEvent.GuildRemoved(GARAGE))
        assertNull(s.guild(GARAGE))
        assertTrue(s.notices.isEmpty())
        assertTrue(GARAGE !in s.exiting)
    }

    @Test fun `guild_added lists the guild and asks for its channels`() {
        val g = Guild(9, "New", TIM, "NEW", T0)
        val r = Reducer.reduce(s0, Action.Gateway(GatewayUpdate.Event(GatewayEvent.GuildAdded(g))))
        assertTrue(r.state.guilds.any { it.id == 9L })
        assertEquals(listOf(Effect.LoadChannels(9)), r.effects)
        assertEquals(Place.InGuild(HIDEOUT), r.state.place, "the current place is left alone")
    }

    @Test fun `guild_updated replaces the guild`() {
        val s = s0.event(GatewayEvent.GuildUpdated(s0.guild(HIDEOUT)!!.copy(name = "Den", icon = "key")))
        assertEquals("Den", s.guild(HIDEOUT)!!.name)
        assertEquals("key", s.guild(HIDEOUT)!!.icon)
    }

    @Test fun `layout_updated takes newer layouts and ignores older ones`() {
        val cur = s0.layouts.getValue(HIDEOUT)
        val newer = cur.copy(version = cur.version + 1, entries = listOf(LayoutEntry(0, listOf(DEV, GENERAL))))
        val s = s0.event(GatewayEvent.LayoutUpdated(HIDEOUT, newer))
        assertEquals(cur.version + 1, s.layouts.getValue(HIDEOUT).version)
        assertEquals(listOf(DEV, GENERAL), s.layouts.getValue(HIDEOUT).entries.first { it.sectionId == 0L }.channelIds)
        val stale = s.event(GatewayEvent.LayoutUpdated(HIDEOUT, cur))
        assertEquals(cur.version + 1, stale.layouts.getValue(HIDEOUT).version)
        assertTrue(s.layouts.getValue(HIDEOUT).entries.map { it.sectionId }.containsAll(cur.sections.map { it.id }), "normalized: every section has an entry")
    }

    @Test fun `roles_updated replaces the roles by seniority and keeps the version`() {
        val roles = listOf(Role(1, HIDEOUT, "a", "", 0, 0, T0), Role(2, HIDEOUT, "b", "", 5, 0, T0))
        val s = s0.event(GatewayEvent.RolesUpdated(HIDEOUT, roles, 9))
        assertEquals(listOf("b", "a"), s.roles[HIDEOUT]!!.map { it.name })
        assertEquals(9L, s.roleVersions[HIDEOUT])
    }

    @Test fun `permissions follow owner, roles and administrator`() {
        val tim = s0.perms(HIDEOUT)
        assertTrue(Perm.has(tim, Perm.BAN_MEMBERS))
        val ana = s0.copy(me = s0.users[ANA], roles = mapOf(HIDEOUT to listOf(Role(1, HIDEOUT, "mod", "", 1, Perm.MANAGE_CHANNELS, T0), Role(2, HIDEOUT, "admin", "", 2, Perm.ADMINISTRATOR, T0))),
            memberRoles = mapOf(HIDEOUT to mapOf(ANA to listOf(1L))))
        assertTrue(Perm.has(ana.perms(HIDEOUT), Perm.MANAGE_CHANNELS))
        assertFalse(Perm.has(ana.perms(HIDEOUT), Perm.BAN_MEMBERS))
        assertTrue(Perm.has(ana.perms(HIDEOUT), Perm.CONNECT), "the voice baseline")
        val admin = ana.copy(memberRoles = mapOf(HIDEOUT to mapOf(ANA to listOf(2L))))
        assertTrue(Perm.has(admin.perms(HIDEOUT), Perm.BAN_MEMBERS))
    }

    @Test fun `user_updated updates me, dm participants and the user map`() {
        val tim = s0.me!!.copy(avatar = "new-avatar")
        val s = s0.event(GatewayEvent.UserUpdated(tim)).event(GatewayEvent.UserUpdated(s0.users.getValue(ANA).copy(avatar = "ana-av")))
        assertEquals("new-avatar", s.me!!.avatar)
        assertEquals("ana-av", s.dm(DM_ANA)!!.participants.first { it.id == ANA }.avatar)
        assertEquals("ana-av", s.users.getValue(ANA).avatar)
    }

    @Test fun `emojis_updated stores the list sorted by name`() {
        val e = { id: Long, n: String -> Emoji(id, HIDEOUT, n, false, "k$id", TIM, T0) }
        val s = s0.event(GatewayEvent.EmojisUpdated(HIDEOUT, listOf(e(2, "zed"), e(1, "alpha"))))
        assertEquals(listOf("alpha", "zed"), s.emojis[HIDEOUT]!!.map { it.name })
    }

    @Test fun `dm_opened adds the conversation and its people once`() {
        val newbie = User(55, "newbie", T0)
        val dm = DmChannel(66, listOf(s0.me!!, newbie), T0)
        val s = s0.event(GatewayEvent.DmOpened(dm)).event(GatewayEvent.DmOpened(dm))
        assertEquals(1, s.dms.count { it.id == 66L })
        assertEquals(newbie, s.users[55])
    }

    @Test fun `notification_settings_updated from another session is kept, the latest wins`() {
        assertNull(s0.notificationSettings)
        val first = NotificationSettings(
            dmsEnabled = true, guildDefault = NotificationLevel.Mentions, showPreviews = false,
            guilds = listOf(NotificationOverride(HIDEOUT, NotificationLevel.All)),
            channels = listOf(NotificationOverride(DEV, NotificationLevel.Nothing, T0.plusSeconds(3600))),
        )
        val s = s0.event(GatewayEvent.NotificationSettingsUpdated(first))
        assertEquals(first, s.notificationSettings)
        val second = first.copy(dmsEnabled = false, channels = emptyList())
        assertEquals(second, s.event(GatewayEvent.NotificationSettingsUpdated(second)).notificationSettings)
        assertEquals(s0.guilds, s.guilds, "nothing else changes")
    }

    @Test fun `notification settings map from the wire, including the Event payload`() {
        val pb = com.thehideout.v1.NotificationSettings.newBuilder()
            .setDmsEnabled(true).setGuildDefault(com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_MENTIONS).setShowPreviews(true)
            .addGuilds(com.thehideout.v1.NotificationOverride.newBuilder().setId(7).setLevel(com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_NOTHING)
                .setMutedUntil(com.google.protobuf.Timestamp.newBuilder().setSeconds(1_800_000_000)))
            .addChannels(com.thehideout.v1.NotificationOverride.newBuilder().setId(8).setLevel(com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_INHERIT))
            .build()
        val event = com.thehideout.v1.Event.newBuilder().setSequence(9).setNotificationSettingsUpdated(pb).build().toModel()
        assertEquals(
            GatewayEvent.NotificationSettingsUpdated(
                NotificationSettings(
                    true, NotificationLevel.Mentions, true,
                    guilds = listOf(NotificationOverride(7, NotificationLevel.Nothing, java.time.Instant.ofEpochSecond(1_800_000_000))),
                    channels = listOf(NotificationOverride(8, NotificationLevel.Inherit)),
                ),
            ),
            event,
        )
    }

    // ---- voice ----

    @Test fun `voice_state_update joins, moves between guilds and leaves`() {
        val here = VoiceState(HIDEOUT, LOUNGE, ANA, joinedAt = T0)
        var s = s0.event(GatewayEvent.VoiceStateUpdate(HIDEOUT, ANA, here))
        assertEquals(listOf(here), s.voiceChannelStates(HIDEOUT, LOUNGE))
        val moved = here.copy(channelId = GAME_NIGHT)
        s = s.event(GatewayEvent.VoiceStateUpdate(HIDEOUT, ANA, moved))
        assertEquals(emptyList<VoiceState>(), s.voiceChannelStates(HIDEOUT, LOUNGE))
        assertEquals(listOf(moved), s.voiceChannelStates(HIDEOUT, GAME_NIGHT))
        val elsewhere = VoiceState(GARAGE, 20, ANA, joinedAt = T0)
        s = s.event(GatewayEvent.VoiceStateUpdate(GARAGE, ANA, elsewhere))
        assertNull(s.voice[HIDEOUT]!![ANA], "a user is in one voice channel at a time")
        s = s.event(GatewayEvent.VoiceStateUpdate(GARAGE, ANA, null))
        assertNull(s.voice[GARAGE]!![ANA])
    }

    @Test fun `voice_snapshot replaces the listed guilds and keeps the others`() {
        val old = VoiceState(HIDEOUT, LOUNGE, ANA, joinedAt = T0)
        val keep = VoiceState(GARAGE, 20, BRUNO, joinedAt = T0)
        val fresh = VoiceState(HIDEOUT, LOUNGE, CVETKA, joinedAt = T0)
        val s = s0.event(GatewayEvent.VoiceStateUpdate(HIDEOUT, ANA, old)).event(GatewayEvent.VoiceStateUpdate(GARAGE, BRUNO, keep))
            .event(GatewayEvent.VoiceSnapshot(listOf(HIDEOUT), listOf(fresh)))
        assertEquals(setOf(CVETKA), s.voice[HIDEOUT]!!.keys)
        assertEquals(setOf(BRUNO), s.voice[GARAGE]!!.keys)
        val emptied = s.event(GatewayEvent.VoiceSnapshot(listOf(HIDEOUT), emptyList()))
        assertTrue(emptied.voice[HIDEOUT]!!.isEmpty(), "a listed guild with nobody in voice is emptied")
    }

    @Test fun `a new stream clears the voice states`() {
        val s = s0.event(GatewayEvent.VoiceStateUpdate(HIDEOUT, ANA, VoiceState(HIDEOUT, LOUNGE, ANA, joinedAt = T0)))
            .reduce(Action.Gateway(GatewayUpdate.Connected))
        assertTrue(s.voice.isEmpty())
    }

    @Test fun `voice_moved does nothing while this device is not in voice`() = assertEquals(s0, s0.event(GatewayEvent.VoiceMoved(HIDEOUT, LOUNGE)))

    // ---- optimistic sends ----

    @Test fun `send queues a pending message with a nonce and asks the server`() {
        val s = s0.withLog(GENERAL, listOf(1))
        val r = Reducer.reduce(s, Action.Send("  hello  ", T0))
        val out = r.state.outgoing.single()
        assertEquals("hello", out.content)
        assertEquals(Delivery.Pending, out.state)
        assertEquals("$TIM-1", out.nonce)
        assertEquals(listOf(Effect.SendMessage(GENERAL, "hello", out.nonce, emptyList())), r.effects)
    }

    @Test fun `empty and oversized text is not sent`() {
        assertEquals(s0, s0.reduce(Action.Send("   ", T0)))
        assertEquals(s0, s0.reduce(Action.Send("x".repeat(MAX_MESSAGE_CHARS + 1), T0)))
        assertEquals(1, s0.reduce(Action.Send("x".repeat(MAX_MESSAGE_CHARS), T0)).outgoing.size)
    }

    @Test fun `the reply and the gateway echo make one message, in either order`() {
        val sent = s0.withLog(GENERAL, listOf(1)).reduce(Action.Send("hi", T0))
        val nonce = sent.outgoing.single().nonce
        val m = msg(2, GENERAL, TIM, "hi", nonce)
        val replyFirst = sent.reduce(Action.MessageSent(GENERAL, nonce, Result.success(m))).event(GatewayEvent.MessageCreated(m))
        assertEquals(listOf(1L, 2L), replyFirst.log(GENERAL)!!.messages.map { it.id })
        assertTrue(replyFirst.outgoing.isEmpty())
        val echoFirst = sent.event(GatewayEvent.MessageCreated(m))
        assertTrue(echoFirst.outgoing.isEmpty(), "the echo reconciles by nonce")
        val both = echoFirst.reduce(Action.MessageSent(GENERAL, nonce, Result.success(m)))
        assertEquals(listOf(1L, 2L), both.log(GENERAL)!!.messages.map { it.id })
    }

    @Test fun `a failed send waits for retry and a transient failure stays quiet`() {
        val sent = s0.withLog(GENERAL, listOf(1)).reduce(Action.Send("hi", T0))
        val nonce = sent.outgoing.single().nonce
        val flaky = sent.reduce(Action.MessageSent(GENERAL, nonce, Result.failure(AppError.Unavailable("reset"))))
        assertEquals(Delivery.Failed, flaky.outgoing.single().state)
        assertTrue(flaky.notices.isEmpty())
        val refused = sent.reduce(Action.MessageSent(GENERAL, nonce, Result.failure(AppError.PermissionDenied)))
        assertEquals("Message not sent. Permission denied.", refused.notices.single().text)
        val retried = Reducer.reduce(flaky, Action.Retry(nonce))
        assertEquals(Delivery.Pending, retried.state.outgoing.single().state)
        assertEquals(listOf(Effect.SendMessage(GENERAL, "hi", nonce, emptyList())), retried.effects)
        assertEquals(emptyList<Effect>(), Reducer.reduce(retried.state, Action.Retry(nonce)).effects, "a pending send is not retried")
    }

    @Test fun `nonces keep counting so two sends never share one`() {
        val s = s0.reduce(Action.Send("a", T0)).reduce(Action.Send("b", T0))
        assertEquals(listOf("$TIM-1", "$TIM-2"), s.outgoing.map { it.nonce })
    }

    // ---- pagination ----

    @Test fun `the first page fills the log and older pages prepend until the end`() {
        val page1 = MessagePage(listOf(5, 4, 3).map { msg(it.toLong(), GENERAL, ANA) }, hasMore = true)
        var s = s0.reduce(Action.MessagesLoaded(GENERAL, null, Result.success(page1)))
        assertEquals(listOf(3L, 4L, 5L), s.log(GENERAL)!!.messages.map { it.id })
        assertTrue(s.log(GENERAL)!!.loaded); assertTrue(s.log(GENERAL)!!.hasMore)
        val older = Reducer.reduce(s, Action.LoadOlder(GENERAL))
        assertEquals(listOf(Effect.LoadMessages(GENERAL, 3)), older.effects)
        assertEquals(Fetch.Older, older.state.log(GENERAL)!!.fetch)
        assertEquals(emptyList<Effect>(), Reducer.reduce(older.state, Action.LoadOlder(GENERAL)).effects, "one older fetch at a time")
        s = older.state.reduce(Action.MessagesLoaded(GENERAL, 3, Result.success(MessagePage(listOf(2, 1).map { msg(it.toLong(), GENERAL, ANA) }, hasMore = false))))
        assertEquals(listOf(1L, 2L, 3L, 4L, 5L), s.log(GENERAL)!!.messages.map { it.id })
        assertFalse(s.log(GENERAL)!!.hasMore)
        assertEquals(Fetch.Idle, s.log(GENERAL)!!.fetch)
        assertEquals(1L, s.log(GENERAL)!!.oldestId)
        assertEquals(emptyList<Effect>(), Reducer.reduce(s, Action.LoadOlder(GENERAL)).effects, "nothing older to ask for")
    }

    @Test fun `a failed older page keeps the log and says so`() {
        val s = s0.withLog(GENERAL, listOf(3, 4), hasMore = true).reduce(Action.LoadOlder(GENERAL))
            .reduce(Action.MessagesLoaded(GENERAL, 3, Result.failure(AppError.Unavailable("offline"))))
        assertEquals(Fetch.Idle, s.log(GENERAL)!!.fetch)
        assertEquals(2, s.log(GENERAL)!!.messages.size)
        assertTrue(s.notices.single().text.startsWith("Couldn't load messages."))
    }

    @Test fun `a refreshed newest page merges, takes the page's reactions and keeps older history`() {
        val up = Unicode("👍")
        val base = s0.withLog(GENERAL, listOf(1, 2, 3))
        val fresh = MessagePage(listOf(msg(4, GENERAL, ANA), msg(3, GENERAL, ANA).copy(reactions = listOf(Reaction(up, 2, false)))), hasMore = true)
        val s = base.reduce(Action.MessagesLoaded(GENERAL, null, Result.success(fresh)))
        assertEquals(listOf(1L, 2L, 3L, 4L), s.log(GENERAL)!!.messages.map { it.id })
        assertEquals(2, s.log(GENERAL)!!.messages[2].reactions.single().count)
    }

    @Test fun `a refreshed page that leaves a gap replaces the log`() {
        val s = s0.withLog(GENERAL, listOf(1, 2), hasMore = false)
            .reduce(Action.MessagesLoaded(GENERAL, null, Result.success(MessagePage(listOf(60, 59).map { msg(it.toLong(), GENERAL, ANA) }, hasMore = true))))
        assertEquals(listOf(59L, 60L), s.log(GENERAL)!!.messages.map { it.id })
        assertTrue(s.log(GENERAL)!!.hasMore)
    }

    @Test fun `strangers on a page ask for the server's members once`() {
        val stranger = msg(1, GENERAL, 4242)
        val r = Reducer.reduce(
            s0.copy(membersAsked = emptySet(), members = emptyMap()),
            Action.MessagesLoaded(GENERAL, null, Result.success(MessagePage(listOf(stranger, msg(2, GENERAL, 4242)), false))),
        )
        assertEquals(listOf(Effect.LoadMembers(HIDEOUT)), r.effects.distinct())
        assertEquals(1, r.effects.count { it == Effect.LoadMembers(HIDEOUT) }, "asked once, not per message")
    }

    @Test fun `a pending New divider resolves when its history arrives`() {
        val s = s0.event(GatewayEvent.MessageCreated(msg(100, DEV, ANA))).event(GatewayEvent.MessageCreated(msg(101, DEV, ANA)))
            .reduce(Action.OpenChannel(DEV))
        assertEquals(Marker.Pending(DEV, 2), s.marker)
        val resolved = s.reduce(Action.MessagesLoaded(DEV, null, Result.success(MessagePage(listOf(101, 100, 99, 98).map { msg(it.toLong(), DEV, ANA) }, false))))
        assertEquals(Marker.After(DEV, 99L), resolved.marker)
    }

    // ---- the Interrupted refetch ----

    @Test fun `a drop marks the stream interrupted and the next connect refetches the lists and the open channel`() {
        val loaded = s0.withLog(GENERAL, listOf(1, 2)).withLog(DEV, listOf(7))
        val dropped = loaded.reduce(Action.Gateway(GatewayUpdate.Disconnected(Duration.ofSeconds(2))))
        assertEquals(StreamState.Interrupted, dropped.stream)
        assertEquals(ConnectionStatus.Reconnecting, dropped.connection)
        val retrying = dropped.reduce(Action.Gateway(GatewayUpdate.Connecting))
        assertEquals(ConnectionStatus.Connecting, retrying.connection)
        assertEquals(StreamState.Interrupted, retrying.stream, "Connecting overwrites the status, so the stream state remembers the gap")
        val back = Reducer.reduce(retrying, Action.Gateway(GatewayUpdate.Connected))
        assertEquals(listOf(Effect.Refresh, Effect.LoadMessages(GENERAL, null)), back.effects)
        assertEquals(StreamState.Live, back.state.stream)
        assertEquals(ConnectionStatus.Connected, back.state.connection)
        assertTrue(back.state.log(DEV)!!.stale, "other loaded channels refresh on their next visit")
        assertFalse(back.state.log(GENERAL)!!.stale)
        val visit = Reducer.reduce(back.state, Action.OpenChannel(DEV))
        assertEquals(listOf(Effect.LoadMessages(DEV, null)), visit.effects)
        assertFalse(visit.state.log(DEV)!!.stale)
    }

    @Test fun `a first connect fetches nothing`() {
        val r = Reducer.reduce(s0, Action.Gateway(GatewayUpdate.Connected))
        assertEquals(emptyList<Effect>(), r.effects)
    }

    @Test fun `a failed refresh of a loaded channel is tried again on the next visit`() {
        val s = s0.withLog(GENERAL, listOf(1)).reduce(Action.MessagesLoaded(GENERAL, null, Result.failure(AppError.Unavailable("x"))))
        assertTrue(s.log(GENERAL)!!.stale)
    }

    @Test fun `a refresh keeps the place, drops what vanished and asks again for the open server's lists`() {
        val gone = Initial2(s0.guilds.filter { it.id != GARAGE })
        val r = Reducer.reduce(s0, Action.Refreshed(gone))
        assertNull(r.state.guild(GARAGE))
        assertEquals(Place.InGuild(HIDEOUT), r.state.place)
        assertEquals(GENERAL, r.state.currentChannel)
        assertTrue(Effect.LoadMembers(HIDEOUT) in r.effects)
        assertTrue(Effect.LoadAccess(HIDEOUT) in r.effects)
        val away = Reducer.reduce(s0, Action.Refreshed(Initial2(s0.guilds.filter { it.id != HIDEOUT })))
        assertEquals(Place.Home, away.state.place, "the open server is gone: go home")
    }

    private fun Initial2(guilds: List<Guild>) = app.thehideout.core.api.Initial(
        me = s0.me!!,
        guilds = guilds,
        channels = guilds.flatMap { g -> s0.channelsOf(g.id) },
        layouts = guilds.associate { it.id to s0.layouts.getValue(it.id) },
        dms = s0.dms,
    )

    @Test fun `an expired token signs out with the reason`() {
        val s = s0.reduce(Action.Gateway(GatewayUpdate.AuthExpired))
        assertEquals(SessionPhase.SignedOut(AppError.Unauthenticated), s.phase)
        assertTrue(s.guilds.isEmpty() && s.logs.isEmpty())
    }

    // ---- session phases ----

    @Test fun `gates replace the shell without losing the data`() {
        val s = s0.reduce(Action.SetPhase(SessionPhase.Suspended("spam", null)))
        assertEquals(SessionPhase.Suspended("spam", null), s.phase)
        assertEquals(s0.guilds, s.guilds)
        assertEquals(SessionPhase.UpdateRequired, s0.reduce(Action.SetPhase(SessionPhase.UpdateRequired)).phase)
        assertEquals(SessionPhase.TermsRequired("2026-02"), s0.reduce(Action.SetPhase(SessionPhase.TermsRequired("2026-02"))).phase)
    }

    @Test fun `members and access replies fill the lists, failures allow asking again`() {
        val u = User(ANA, "ana", T0)
        val s = s0.reduce(Action.MembersLoaded(HIDEOUT, Result.success(GuildMembers(listOf(u), listOf(Member(u, listOf(1003), T0))))))
        assertEquals(listOf(ANA), s.members[HIDEOUT]); assertEquals(listOf(1003L), s.memberRoles[HIDEOUT]!![ANA])
        val roles = s.reduce(Action.AccessLoaded(HIDEOUT, Result.success(GuildRoles(listOf(Role(1003, HIDEOUT, "Designer", "pink", 2, 0, T0)), 4))))
        assertEquals(4L, roles.roleVersions[HIDEOUT])
        val failed = s0.reduce(Action.MembersLoaded(GARAGE, Result.failure(AppError.Unavailable("x"))))
        assertFalse(GARAGE in failed.membersAsked)
    }

    @Test fun `notices queue three and dismiss by id`() {
        var s = s0
        repeat(5) { s = s.reduce(Action.MessagesLoaded(GENERAL, null, Result.failure(AppError.Other("boom $it")))) }
        // Only a channel with a log can fail a load; make one first.
        s = s0.withLog(GENERAL, listOf(1))
        repeat(5) { s = s.reduce(Action.MessagesLoaded(GENERAL, null, Result.failure(AppError.Other("boom $it")))) }
        assertEquals(3, s.notices.size)
        val first = s.notices.first()
        assertFalse(s.reduce(Action.DismissNotice(first.id)).notices.any { it.id == first.id })
    }

    @Test fun `mentions match whole words only`() {
        assertTrue(mentions("hi @Tim!", "tim"))
        assertTrue(mentions("@tim", "tim"))
        assertFalse(mentions("hi @timothy", "tim"))
        assertFalse(mentions("mail me at x@tim.com", "tim"))
        assertFalse(mentions("tim", "tim"))
    }
}
