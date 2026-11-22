package app.thehideout.core

/** Where the session token lives. The app's implementation is Android-Keystore backed, never plain prefs. */
interface TokenStore {
    fun load(): String?
    fun save(token: String)
    fun clear()
}

class InMemoryTokenStore(private var token: String? = null) : TokenStore {
    override fun load(): String? = token
    override fun save(token: String) { this.token = token }
    override fun clear() { token = null }
}
