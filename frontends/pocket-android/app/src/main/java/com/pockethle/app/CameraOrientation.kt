package com.pockethle.app

import java.nio.ByteBuffer

/** Rotate copied YUV planes before CAM1 preview/tracking consumes them.
 * No mirroring: display orientation and lens mirroring are separate concerns. */
internal object CameraOrientation {
    fun turns(sensorDegrees: Int, displayDegrees: Int, frontFacing: Boolean): Int {
        val relative = sensorDegrees + if (frontFacing) displayDegrees else -displayDegrees
        return ((relative % 360 + 360) % 360) / 90
    }

    fun copyPlane(out: ByteBuffer, source: ByteBuffer, rowStride: Int,
                  pixelStride: Int, left: Int, top: Int, width: Int, height: Int,
                  turns: Int) {
        val base = source.position() + top * rowStride + left * pixelStride
        val outputWidth = if (turns % 2 == 0) width else height
        val outputHeight = if (turns % 2 == 0) height else width
        for (y in 0 until outputHeight) for (x in 0 until outputWidth) {
            val sx = when (turns) { 1 -> y; 2 -> width - 1 - x; 3 -> width - 1 - y; else -> x }
            val sy = when (turns) { 1 -> height - 1 - x; 2 -> height - 1 - y; 3 -> x; else -> y }
            out.put(source.get(base + sy * rowStride + sx * pixelStride))
        }
    }
}
