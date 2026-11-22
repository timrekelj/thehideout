package si.zabit.thehideout

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.core.content.edit
import app.thehideout.core.TokenStore
import app.thehideout.core.crypto.AesGcmBox
import java.security.KeyStore
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

/**
 * The session token, sealed with AES-256-GCM under a non-exportable Android Keystore key. Only the
 * ciphertext reaches storage; a value that cannot be opened (a restored backup, a reset keystore)
 * reads as signed out.
 */
class KeystoreTokenStore(context: Context) : TokenStore {
    private val prefs = context.getSharedPreferences("session", Context.MODE_PRIVATE)
    private val box = AesGcmBox(::key)

    override fun load(): String? {
        val sealed = prefs.getString(KEY_TOKEN, null) ?: return null
        val plain = runCatching { Base64.decode(sealed, Base64.NO_WRAP) }.getOrNull()?.let(box::open)
        if (plain == null) {
            clear()
            return null
        }
        return String(plain, Charsets.UTF_8)
    }

    override fun save(token: String) {
        val sealed = Base64.encodeToString(box.seal(token.toByteArray(Charsets.UTF_8)), Base64.NO_WRAP)
        prefs.edit { putString(KEY_TOKEN, sealed) }
    }

    override fun clear() {
        prefs.edit { remove(KEY_TOKEN) }
    }

    private fun key(): SecretKey {
        val store = KeyStore.getInstance(PROVIDER).apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    private companion object {
        const val PROVIDER = "AndroidKeyStore"
        const val ALIAS = "thehideout.session"
        const val KEY_TOKEN = "token"
    }
}
