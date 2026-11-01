package com.pockethle.app

import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FramePayloadTest {
    private fun payload(vararg words: Int, pixelBytes: Int = 0): ByteArray =
        ByteBuffer.allocate(words.size * 4 + pixelBytes).order(ByteOrder.LITTLE_ENDIAN).apply {
            words.forEach { putInt(it) }
        }.array()

    @Test fun nativeFramesRetainTheOriginalWireFormat() {
        val frame = decodeFramePayload(payload(2, 1, pixelBytes = 8))!!
        assertEquals(FramePayload(2, 1, 2, 1, 8), frame)
    }

    @Test fun highResolutionDoesNotChangeGuestCoordinates() {
        val frame = decodeFramePayload(payload(-1, 2, 1, 4, 2, pixelBytes = 32))!!
        assertEquals(FramePayload(2, 1, 4, 2, 20), frame)
    }

    @Test fun malformedAndTruncatedImagesAreRejected() {
        assertNull(decodeFramePayload(ByteArray(7)))
        assertNull(decodeFramePayload(payload(-1, 240)))
        assertNull(decodeFramePayload(payload(0, 320)))
        assertNull(decodeFramePayload(payload(240, 320)))
        assertNull(decodeFramePayload(payload(Int.MAX_VALUE, Int.MAX_VALUE)))
        assertNull(decodeFramePayload(payload(-1, 2, 1, 3, 2, pixelBytes = 32)))
        assertNull(decodeFramePayload(payload(-1, 2, 1, 4, 2, pixelBytes = 31)))
    }
}
