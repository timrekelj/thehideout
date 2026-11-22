package app.thehideout.core

import app.thehideout.core.crypto.AesGcmBox
import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

class AesGcmBoxTest {
    private fun key(): SecretKey = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()

    @Test fun `a sealed token opens to itself and is not the plain text`() {
        val k = key()
        val box = AesGcmBox { k }
        val sealed = box.seal("session-token-123".toByteArray())
        assertFalse(String(sealed, Charsets.ISO_8859_1).contains("session-token"))
        assertEquals("session-token-123", String(box.open(sealed)!!))
    }

    @Test fun `sealing twice gives different bytes (a fresh IV each time)`() {
        val k = key()
        val box = AesGcmBox { k }
        assertFalse(box.seal("x".toByteArray()).contentEquals(box.seal("x".toByteArray())))
    }

    @Test fun `tampering, truncation and another key all read as nothing`() {
        val k = key()
        val box = AesGcmBox { k }
        val sealed = box.seal("secret".toByteArray())
        val flipped = sealed.copyOf().also { it[it.size - 1] = (it[it.size - 1].toInt() xor 1).toByte() }
        assertNull(box.open(flipped))
        assertNull(box.open(sealed.copyOf(8)))
        assertNull(box.open(ByteArray(0)))
        val other = key()
        assertNull(AesGcmBox { other }.open(sealed))
    }
}
