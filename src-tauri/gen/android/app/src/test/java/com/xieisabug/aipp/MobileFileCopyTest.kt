package com.xieisabug.aipp

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream

class MobileFileCopyTest {
    private val reserve = 1024L * 1024

    @Test
    fun test_copy_preserves_bytes_at_limits() {
        val bytes = ByteArray(70000) { (it % 251).toByte() }
        val output = ByteArrayOutputStream()
        val copied = copyMobileFile(ByteArrayInputStream(bytes), output, bytes.size.toLong(), bytes.size.toLong() + 10, 10) { Long.MAX_VALUE }
        assertEquals(bytes.size.toLong(), copied)
        assertArrayEquals(bytes, output.toByteArray())
    }

    @Test
    fun test_copy_rejects_exact_safety_reserve_boundary() {
        val output = ByteArrayOutputStream()
        assertThrows(IllegalStateException::class.java) {
            copyMobileFile(ByteArrayInputStream(byteArrayOf(1, 2)), output, 2, 2, 0) { reserve + 2 }
        }
        assertEquals(0, output.size())
    }

    @Test
    fun test_copy_rejects_space_drop_mid_stream() {
        val output = ByteArrayOutputStream()
        var calls = 0
        assertThrows(IllegalStateException::class.java) {
            copyMobileFile(ByteArrayInputStream(ByteArray(65537)), output, 70000, 70000, 0) {
                if (calls++ == 0) Long.MAX_VALUE else reserve + 1
            }
        }
        assertEquals(65536, output.size())
    }

    @Test
    fun test_copy_propagates_read_failure_mid_stream() {
        val input = object : InputStream() {
            var called = false
            override fun read(): Int = throw UnsupportedOperationException()
            override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
                if (called) throw IOException("provider read failed")
                called = true
                buffer[offset] = 42
                return 1
            }
        }
        val output = ByteArrayOutputStream()
        val error = assertThrows(IOException::class.java) { copyMobileFile(input, output, 10, 10, 0) { Long.MAX_VALUE } }
        assertEquals("provider read failed", error.message)
        assertArrayEquals(byteArrayOf(42), output.toByteArray())
    }

    @Test
    fun test_copy_rejects_single_file_limit() {
        val output = ByteArrayOutputStream()
        assertThrows(IllegalArgumentException::class.java) {
            copyMobileFile(ByteArrayInputStream(byteArrayOf(1, 2)), output, 1, 10, 0) { Long.MAX_VALUE }
        }
        assertEquals(0, output.size())
    }

    @Test
    fun test_copy_rejects_batch_limit() {
        val output = ByteArrayOutputStream()
        assertThrows(IllegalArgumentException::class.java) {
            copyMobileFile(ByteArrayInputStream(byteArrayOf(1, 2)), output, 10, 10, 9) { Long.MAX_VALUE }
        }
        assertEquals(0, output.size())
    }
}
