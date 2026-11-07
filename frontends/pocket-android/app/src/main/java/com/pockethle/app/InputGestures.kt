package com.pockethle.app

/** Optional guest long actions. Factory bindings are immediate without a long action. */
internal class InputGestures {
    private data class Press(
        val at: Long,
        val short: Int?,
        val long: Int,
        var fired: Boolean = false,
    )

    private val presses = mutableMapOf<String, Press>()
    private val immediate = mutableMapOf<String, Int?>()
    private val pulses = mutableListOf<Pair<Int, Long>>()
    private val held = mutableMapOf<Int, Int>()

    private fun change(vk: Int, down: Boolean, out: MutableList<Pair<Int, Boolean>>) {
        val count = held[vk] ?: 0
        if (down) {
            held[vk] = count + 1
            if (count == 0) out.add(vk to true)
        } else if (count > 0) {
            held[vk] = count - 1
            if (count == 1) out.add(vk to false)
        }
    }

    fun immediateEdge(token: String, down: Boolean, vk: Int?): List<Pair<Int, Boolean>> {
        val out = mutableListOf<Pair<Int, Boolean>>()
        if (down) {
            if (!immediate.containsKey(token)) {
                immediate[token] = vk
                if (vk != null) change(vk, true, out)
            }
        } else if (immediate.containsKey(token))
            immediate.remove(token)?.let { change(it, false, out) }
        return out
    }

    fun edge(
        token: String,
        down: Boolean,
        now: Long,
        short: Int?,
        long: Int,
    ): List<Pair<Int, Boolean>> {
        val late = !down && presses[token]?.let { !it.fired && now - it.at >= HOLD_MS } == true
        val out = tick(now).toMutableList()
        if (down) presses.putIfAbsent(token, Press(now, short, long))
        else
            presses.remove(token)?.let { p ->
                if (p.fired) {
                    if (late) pulses.add(p.long to now + 80) else change(p.long, false, out)
                } else
                    p.short?.let {
                        change(it, true, out)
                        pulses.add(it to now + 80)
                    }
            }
        return out
    }

    fun tick(now: Long): List<Pair<Int, Boolean>> {
        val out = mutableListOf<Pair<Int, Boolean>>()
        presses.values.forEach { p ->
            if (!p.fired && now - p.at >= HOLD_MS) {
                p.fired = true
                change(p.long, true, out)
            }
        }
        pulses.removeAll { (vk, at) ->
            if (now >= at) {
                change(vk, false, out)
                true
            } else false
        }
        return out
    }

    fun cancelPrefix(prefix: String): List<Pair<Int, Boolean>> {
        val out = mutableListOf<Pair<Int, Boolean>>()
        presses.keys
            .filter { it.startsWith(prefix) }
            .forEach { key ->
                presses.remove(key)?.let { if (it.fired) change(it.long, false, out) }
            }
        immediate.keys
            .filter { it.startsWith(prefix) }
            .forEach { out.addAll(immediateEdge(it, false, null)) }
        return out
    }

    fun clear() {
        immediate.clear()
        presses.clear()
        pulses.clear()
        held.clear()
    }

    fun active() = presses.isNotEmpty() || pulses.isNotEmpty()

    companion object {
        const val HOLD_MS = 650L

        fun bindingKey(gizmondo: Boolean, pad: Boolean, input: String) =
            "${if (gizmondo) "gizmondo" else "pocketpc"}:${if (pad) "controller" else "keyboard"}:${if (pad) InputBindings.controllerCanonical(input) else InputBindings.canonical(input).lowercase(java.util.Locale.ROOT)}"
    }
}
