package app.thehideout.ui

import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.store.Action
import app.thehideout.core.store.AppState
import app.thehideout.core.store.ChannelLog
import app.thehideout.core.store.Reducer
import kotlinx.coroutines.runBlocking

/** Like [fakeWorldState] but opened on #design, which the fake backend seeds with every attachment kind. */
fun attachmentWorldState(): AppState = runBlocking {
    val fake = FakeBackend()
    var s = Reducer.reduce(AppState(), Action.InitialLoaded(fake.loadInitial())).state
    fun AppState.reduce(a: Action) = Reducer.reduce(this, a).state
    s = s.copy(logs = s.logs + (FakeBackend.DESIGN to ChannelLog()))
    s = s.reduce(Action.MessagesLoaded(FakeBackend.DESIGN, null, Result.success(fake.listMessages(FakeBackend.DESIGN, null))))
    s = s.reduce(Action.OpenChannel(FakeBackend.DESIGN))
    s = s.reduce(Action.MembersLoaded(FakeBackend.HIDEOUT, Result.success(fake.listMembers(FakeBackend.HIDEOUT))))
    s = s.reduce(Action.AccessLoaded(FakeBackend.HIDEOUT, Result.success(fake.listRoles(FakeBackend.HIDEOUT))))
    s
}
