package app.thehideout.ui

import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.model.GatewayEvent
import app.thehideout.core.model.GatewayUpdate
import app.thehideout.core.store.Action
import app.thehideout.core.store.AppState
import app.thehideout.core.store.Reducer
import kotlinx.coroutines.runBlocking
import java.time.Instant

/**
 * The state the app has a minute after launch on the fake world: signed in on The Hideout, #general's
 * newest page loaded, members and roles known, two people in voice, and a few unread messages (one
 * mentions tim) in channels nobody has open. Built by running the real reducer on the fake server's
 * answers, so screenshots show what the app would.
 */
fun fakeWorldState(): AppState = runBlocking {
    val fake = FakeBackend()
    var s = Reducer.reduce(AppState(), Action.InitialLoaded(fake.loadInitial())).state
    fun AppState.reduce(a: Action) = Reducer.reduce(this, a).state
    s = s.reduce(Action.MessagesLoaded(FakeBackend.GENERAL, null, Result.success(fake.listMessages(FakeBackend.GENERAL, null))))
    s = s.reduce(Action.MembersLoaded(FakeBackend.HIDEOUT, Result.success(fake.listMembers(FakeBackend.HIDEOUT))))
    s = s.reduce(Action.AccessLoaded(FakeBackend.HIDEOUT, Result.success(fake.listRoles(FakeBackend.HIDEOUT))))
    // DM history too, so the list can preview the last message.
    for (dm in listOf(FakeBackend.DM_ANA, FakeBackend.DM_BRUNO)) {
        // A page only lands in a log that exists (the app creates it when a channel is first opened).
        s = s.copy(logs = s.logs + (dm to app.thehideout.core.store.ChannelLog()))
        s = s.reduce(Action.MessagesLoaded(dm, null, Result.success(fake.listMessages(dm, null))))
    }
    // #dev's history too, loaded before its mention arrives below, so the Mentions feed has it.
    s = s.copy(logs = s.logs + (FakeBackend.DEV to app.thehideout.core.store.ChannelLog()))
    s = s.reduce(Action.MessagesLoaded(FakeBackend.DEV, null, Result.success(fake.listMessages(FakeBackend.DEV, null))))
    fun event(e: GatewayEvent) { s = s.reduce(Action.Gateway(GatewayUpdate.Event(e))) }
    s = s.reduce(Action.Gateway(GatewayUpdate.Connected))
    event(GatewayEvent.VoiceSnapshot(listOf(FakeBackend.HIDEOUT), fake.voiceSnapshotStates()))
    val at = Instant.parse("2026-11-22T10:01:00Z")
    event(GatewayEvent.MessageCreated(app.thehideout.core.model.Message(20_001, FakeBackend.DEV, FakeBackend.HIDEOUT, FakeBackend.BRUNO, "Gateway backoff now resets on a clean connect, @tim can you review?", at)))
    event(GatewayEvent.MessageCreated(app.thehideout.core.model.Message(20_002, FakeBackend.DEV, FakeBackend.HIDEOUT, FakeBackend.ANA, "Added a test for the silence timeout too.", at)))
    event(GatewayEvent.MessageCreated(app.thehideout.core.model.Message(20_003, FakeBackend.DESIGN, FakeBackend.HIDEOUT, FakeBackend.CVETKA, "New icon set is in the wiki.", at)))
    event(GatewayEvent.MessageCreated(app.thehideout.core.model.Message(20_004, FakeBackend.DM_ANA, null, FakeBackend.ANA, "got a minute later tonight?", at)))
    s
}
