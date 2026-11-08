package com.pockethle.app

/** Per-renderer aggregate; called only on the GL thread. Timings are inclusive. */
internal class GraphicsTiming(private val enabled: Boolean, private val emit: (String) -> Unit) {
    enum class Stage { PREPARE, UPLOAD, FILTER, DRAW, DRAW_GAP, PRESENT_GPU, SHARED_CONVERT, SHARED_HANDOFF, RGB565_CONVERT, GPU_SKIPPED, GPU_DISJOINT, GPU_OVERFLOW }
    private val calls = LongArray(Stage.entries.size)
    private val total = LongArray(Stage.entries.size)
    private val maximum = LongArray(Stage.entries.size)
    private var window = 0L
    fun start(): Long = if (enabled) System.nanoTime() else 0L
    fun end(stage: Stage, start: Long) { if (enabled) record(stage, System.nanoTime() - start) }
    fun record(stage: Stage, nanos: Long, now: Long = System.nanoTime()) {
        if (!enabled) return
        if (window == 0L) window = now
        val i = stage.ordinal
        calls[i]++; total[i] += nanos; maximum[i] = maxOf(maximum[i], nanos)
        if (now - window < 1_000_000_000L) return
        for (entry in Stage.entries) {
            val j = entry.ordinal
            if (calls[j] != 0L) emit("GPU_DIAG domain=android window_ms=${(now-window)/1e6} stage=${entry.name.lowercase()} calls=${calls[j]} total_ms=${total[j]/1e6} mean_ms=${total[j]/1e6/calls[j]} max_ms=${maximum[j]/1e6}")
        }
        calls.fill(0); total.fill(0); maximum.fill(0); window = now
    }
}
