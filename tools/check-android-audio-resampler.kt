package com.pockethle.app

fun main() {
    check(AudioResampler(2, 4, 1).convert(shortArrayOf(0, 1000, 2000))
        .contentEquals(shortArrayOf(0, 500, 1000, 1500, 2000)))
    check(AudioResampler(2, 4, 2).convert(shortArrayOf(0, 1000, 1000, 0))
        .contentEquals(shortArrayOf(0, 1000, 500, 500, 1000, 0)))
    val original = shortArrayOf(-32768, 0, 32767)
    check(AudioResampler(48000, 48000, 1).convert(original) === original)
    for ((source, output) in listOf(22050 to 48000, 44100 to 48000, 48000 to 22050)) {
        for (channels in 1..2) {
            val input = ShortArray(10001 * channels) { ((it * 53) % 60000 - 30000).toShort() }
            val whole = AudioResampler(source, output, channels).convert(input)
            val streaming = AudioResampler(source, output, channels)
            val parts = ArrayList<Short>()
            var offset = 0
            var chunk = 1
            while (offset < input.size) {
                val end = minOf(input.size, offset + chunk * channels)
                parts.addAll(streaming.convert(input.copyOfRange(offset, end)).toList())
                offset = end
                chunk = (chunk * 7 % 221) + 1
            }
            check(whole.contentEquals(parts.toShortArray())) { "Chunk boundary mismatch $source->$output/$channels" }
            val expectedFrames = ((10000L * output) / source + 1).toInt()
            check(whole.size == expectedFrames * channels)
        }
    }
    println("PASS: known interpolation, stereo separation, same-rate bypass, streaming boundaries and frame counts")
}
