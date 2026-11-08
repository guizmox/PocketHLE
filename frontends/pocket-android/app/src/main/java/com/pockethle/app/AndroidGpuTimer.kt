package com.pockethle.app

import android.opengl.GLES30.*
import android.util.Log

/** UI-context queries, never shared with the emulation context. ES3 entry points
 * are used for EXT_disjoint_timer_query. JNI reads all 64 result bits; Android
 * has no corresponding Java binding. No result wait or finish.
 */
internal class AndroidGpuTimer(private val timing: GraphicsTiming) {
    private val ids = IntArray(8)
    private val pending = BooleanArray(8)
    private val value = IntArray(1)
    private var active = -1
    private val supported: Boolean
    init {
        val extension = glGetString(GL_EXTENSIONS)?.split(' ')?.contains("GL_EXT_disjoint_timer_query") == true
        if (extension) glGetQueryiv(ELAPSED, 0x8864, value, 0)
        supported = extension && value[0] > 0 && NativeBridge.nativeGpuQueryResult(0) == 0L
        if (supported) { glGenQueries(ids.size, ids, 0); glGetIntegerv(DISJOINT, value, 0) }
        Log.i("PocketHLE", "GPU_DIAG domain=android timer=${if (supported) "EXT_disjoint_timer_query" else "unavailable"} coverage=upload_filter_draw result_bits=64")
    }
    fun begin() {
        if (!supported) return
        glGetIntegerv(DISJOINT, value, 0)
        if (value[0] != 0) { pending.fill(false); timing.record(GraphicsTiming.Stage.GPU_DISJOINT, 0) }
        for (i in ids.indices) {
            if (!pending[i]) continue
            glGetQueryObjectuiv(ids[i], GL_QUERY_RESULT_AVAILABLE, value, 0)
            if (value[0] == 0) continue
            val ns = NativeBridge.nativeGpuQueryResult(ids[i])
            if (ns < 0) timing.record(GraphicsTiming.Stage.GPU_OVERFLOW, 0)
            else timing.record(GraphicsTiming.Stage.PRESENT_GPU, ns)
            pending[i] = false
        }
        active = pending.indexOfFirst { !it }
        if (active >= 0) glBeginQuery(ELAPSED, ids[active])
        else timing.record(GraphicsTiming.Stage.GPU_SKIPPED, 0)
    }
    fun end() {
        if (active < 0) return
        glEndQuery(ELAPSED); pending[active] = true; active = -1
    }
    companion object { private const val ELAPSED = 0x88bf; private const val DISJOINT = 0x8fbb }
}
