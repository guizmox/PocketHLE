package com.pockethle.app

/** Streaming PCM16 conversion; carries timing and the last frame across JNI pulls. */
internal class AudioResampler(
    private val sourceRate: Int,
    private val outputRate: Int,
    private val channels: Int
) {
    private var inputFrames = 0L
    private var nextPosition = 0L // Source-frame position, in units of 1/outputRate.
    private val previous = ShortArray(channels)

    init {
        require(sourceRate > 0 && outputRate > 0 && channels in 1..2)
    }

    fun convert(input: ShortArray): ShortArray {
        require(input.size % channels == 0)
        if (input.isEmpty() || sourceRate == outputRate) return input
        val frames = input.size / channels
        val end = inputFrames + frames
        val capacity = (((frames.toLong() + 1) * outputRate + sourceRate - 1) / sourceRate + 1).toInt()
        val output = ShortArray(capacity * channels)
        var written = 0
        while (true) {
            val frame = nextPosition / outputRate
            val fraction = nextPosition % outputRate
            if (frame >= end || (fraction != 0L && frame + 1 >= end)) break
            for (channel in 0 until channels) {
                fun sample(index: Long): Int = if (index < inputFrames) {
                    previous[channel].toInt()
                } else {
                    input[((index - inputFrames).toInt() * channels) + channel].toInt()
                }
                val first = sample(frame)
                val second = if (fraction == 0L) first else sample(frame + 1)
                output[written++] = ((first.toLong() * (outputRate - fraction) +
                    second.toLong() * fraction) / outputRate).toShort()
            }
            nextPosition += sourceRate
        }
        for (channel in 0 until channels) previous[channel] = input[input.size - channels + channel]
        inputFrames = end
        return output.copyOf(written)
    }
}
