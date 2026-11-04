package com.xieisabug.aipp

import java.io.InputStream
import java.io.OutputStream

/** Copies a picked document while enforcing per-file, batch and storage limits. */
internal fun copyMobileFile(
    input: InputStream,
    output: OutputStream,
    singleLimit: Long,
    batchLimit: Long,
    batchBytes: Long,
    availableSpace: () -> Long,
): Long {
    val buffer = ByteArray(64 * 1024)
    var count = 0L
    while (true) {
        val n = input.read(buffer)
        if (n < 0) break
        count += n
        require(count <= singleLimit && count <= batchLimit - batchBytes) { "document/batch size limit exceeded (20/50 MiB)" }
        check(availableSpace() > n + 1024 * 1024) { "insufficient storage: need 1 MiB safety reserve" }
        output.write(buffer, 0, n)
    }
    output.flush()
    return count
}
