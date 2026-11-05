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
        for (scale in 2..4) {
            val textureWidth = 320 * scale
            val textureHeight = 240 * scale
            val frame = decodeFramePayload(
                payload(-1, 320, 240, textureWidth, textureHeight, pixelBytes = textureWidth * textureHeight * 4),
            )
            assertEquals(FramePayload(320, 240, textureWidth, textureHeight, 20), frame)
        }
    }

    @Test fun malformedAndTruncatedImagesAreRejected() {
        assertNull(decodeFramePayload(ByteArray(7)))
        assertNull(decodeFramePayload(payload(-1, 240)))
        assertNull(decodeFramePayload(payload(0, 320)))
        assertNull(decodeFramePayload(payload(240, 320)))
        assertNull(decodeFramePayload(payload(Int.MAX_VALUE, Int.MAX_VALUE)))
        assertNull(decodeFramePayload(payload(-1, 2, 1, 3, 2, pixelBytes = 32)))
        assertNull(decodeFramePayload(payload(-1, 2, 1, 4, 3, pixelBytes = 48)))
        assertNull(decodeFramePayload(payload(-1, 2, 1, 10, 5, pixelBytes = 200)))
        assertNull(decodeFramePayload(payload(-1, 2, 1, 4, 2, pixelBytes = 31)))
        assertNull(decodeFramePayload(payload(-1, 2, 1, 8, 4, pixelBytes = 127)))
    }
}
