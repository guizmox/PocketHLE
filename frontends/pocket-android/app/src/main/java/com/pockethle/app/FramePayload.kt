package com.pockethle.app

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Guest geometry is independent of the OpenGLES texture's pixel density. */
internal data class FramePayload(
    val width: Int,
    val height: Int,
    val textureWidth: Int,
    val textureHeight: Int,
    val rgbaOffset: Int,
    val sharedToken: Long = 0,
)

internal fun decodeFramePayload(raw: ByteArray): FramePayload? {
    if (raw.size < 8) return null
    val buffer = ByteBuffer.wrap(raw).order(ByteOrder.LITTLE_ENDIAN)
    var width = buffer.int
    var height = buffer.int
    var offset = 8
    var textureWidth = width
    var textureHeight = height
    if (width == -2) {
        if (raw.size != 28) return null
        width = height; height = buffer.int
        textureWidth = buffer.int; textureHeight = buffer.int
        val token = buffer.long
        if (width <= 0 || height <= 0 || token <= 0) return null
        val scale = textureWidth / width
        if (scale !in 1..4 || textureWidth.toLong() != width.toLong()*scale || textureHeight.toLong() != height.toLong()*scale) return null
        return FramePayload(width,height,textureWidth,textureHeight,28,token)
    }
    if (width == -1) {
        if (raw.size < 20) return null
        width = height
        height = buffer.int
        textureWidth = buffer.int
        textureHeight = buffer.int
        offset = 20
    }
    if (width <= 0 || height <= 0 || textureWidth <= 0 || textureHeight <= 0) return null
    if (offset == 20) {
        val scale = textureWidth / width
        if (scale !in 2..4 || textureWidth.toLong() != width.toLong() * scale ||
            textureHeight.toLong() != height.toLong() * scale) return null
    }
    // Reject malformed lengths before copying or asking GL to allocate an image.
    val pixels = textureWidth.toLong() * textureHeight
    if (pixels > (raw.size.toLong() - offset) / 4) return null
    return FramePayload(width, height, textureWidth, textureHeight, offset)
}
