package com.pockethle.app

import org.json.JSONArray
import org.json.JSONObject

/** Edits the existing shared schema; a draft is persisted only by Save. */
internal class InputEditorState(raw: String) {
    val json = JSONObject(raw)
    val keyboard =
        json.optJSONArray("keybindings") ?: JSONArray().also { json.put("keybindings", it) }
    val controller =
        json.optJSONObject("gamepad_bindings")
            ?: InputBindings.defaultController().also { json.put("gamepad_bindings", it) }
    val stylus =
        json.optJSONObject("stylus_stick")
            ?: JSONObject()
                .apply {
                    put("enabled", true)
                    put("speed", 240)
                    put(
                        "keyboard",
                        JSONArray().apply {
                            listOf("O", "L", "K", "M").forEach { put(JSONArray().put(it)) }
                        },
                    )
                    put(
                        "controller",
                        JSONArray().apply {
                            StylusBindings.directions.forEach {
                                put(JSONArray().put("RightStick$it"))
                            }
                        },
                    )
                }
                .also { json.put("stylus_stick", it) }

    fun visibleButtons(gizmondo: Boolean) =
        InputBindings.buttons.keys.filter {
            if (gizmondo) !it.startsWith("pocket_pc_app") else !it.startsWith("giz_piano")
        }

    fun label(target: String?): String =
        when {
                target == null -> "Unassigned"
                target == "host" -> "Screenshot"
                target.startsWith("stylus:") ->
                    "Stylus ${StylusBindings.directions[target.substringAfter(':').toInt()]}"
                else -> InputBindings.buttons[target]?.first ?: target
            }
            .replace("↑", "Up")
            .replace("↓", "Down")
            .replace("←", "Left")
            .replace("→", "Right")

    fun short(target: String?) =
        when {
            target == null -> "—"
            target == "host" -> "Photo"
            target.startsWith("stylus:") ->
                "Pen ${StylusBindings.directions[target.substringAfter(':').toInt()]}"
            target.startsWith("dpad_") ->
                target.substringAfter('_').replaceFirstChar { it.uppercase() }
            target.startsWith("pocket_pc_app") -> "App ${target.last()}"
            target.startsWith("button_") -> target.last().uppercaseChar().toString()
            target == "soft1" -> "Soft 1"
            target == "soft2" -> "Soft 2"
            else -> label(target).substringBefore(" /").substringBefore(" (")
        }

    private fun same(a: String, b: String, pad: Boolean) =
        if (pad) InputBindings.controllerCanonical(a) == InputBindings.controllerCanonical(b)
        else InputBindings.canonical(a).equals(InputBindings.canonical(b), true)

    fun assignment(input: String, pad: Boolean, gizmondo: Boolean): String? {
        if (!pad && input == "F10") return "host"
        if (!gizmondo && stylus.optBoolean("enabled", true)) {
            val directions = stylus.optJSONArray(if (pad) "controller" else "keyboard")
            for (i in 0..3) {
                val keys = directions?.optJSONArray(i) ?: continue
                if ((0 until keys.length()).any { same(keys.getString(it), input, pad) })
                    return "stylus:$i"
            }
        }
        if (pad)
            return InputBindings.controllerAliases(input).firstNotNullOfOrNull {
                controller.optString(it).takeIf(String::isNotEmpty)
            }
        for (i in 0 until keyboard.length()) {
            val binding = keyboard.getJSONObject(i)
            val keys = binding.optJSONArray("keys") ?: continue
            if ((0 until keys.length()).any { same(keys.getString(it), input, false) })
                return binding.getString("button")
        }
        return null
    }

    fun savedInputs(pad: Boolean): List<String> {
        val keys = mutableListOf<String>()
        if (pad) controller.keys().forEach { keys.add(InputBindings.controllerCanonical(it)) }
        else
            for (i in 0 until keyboard.length()) keyboard
                .getJSONObject(i)
                .optJSONArray("keys")
                ?.let { a -> for (j in 0 until a.length()) keys.add(a.getString(j)) }
        stylus.optJSONArray(if (pad) "controller" else "keyboard")?.let { a ->
            for (i in 0 until a.length()) a.optJSONArray(i)?.let { b ->
                for (j in 0 until b.length()) keys.add(b.getString(j))
            }
        }
        return keys.distinct().sorted()
    }

    fun assign(input: String, pad: Boolean, target: String?): Boolean {
        if (!pad && input == "F10") return false
        if (pad)
            controller
                .keys()
                .asSequence()
                .toList()
                .filter { same(it, input, true) }
                .forEach(controller::remove)
        else
            for (i in 0 until keyboard.length()) {
                val entry = keyboard.getJSONObject(i)
                val old = entry.optJSONArray("keys") ?: continue
                entry.put(
                    "keys",
                    JSONArray().apply {
                        for (j in 0 until old.length()) if (!same(old.getString(j), input, false))
                            put(old.getString(j))
                    },
                )
            }
        val kind = if (pad) "controller" else "keyboard"
        val directions = stylus.optJSONArray(kind) ?: JSONArray().also { stylus.put(kind, it) }
        for (i in 0..3) {
            val old = directions.optJSONArray(i) ?: JSONArray()
            directions.put(
                i,
                JSONArray().apply {
                    for (j in 0 until old.length()) if (!same(old.getString(j), input, pad))
                        put(old.getString(j))
                },
            )
        }
        if (pad && same(json.optString("gamepad_fullscreen"), input, true))
            json.put("gamepad_fullscreen", JSONObject.NULL)
        if (target?.startsWith("stylus:") == true) {
            stylus.put("enabled", true)
            directions.getJSONArray(target.substringAfter(':').toInt()).put(input)
        } else if (target != null) {
            if (pad) controller.put(InputBindings.controllerCanonical(input), target)
            else {
                val entry =
                    (0 until keyboard.length())
                        .map { keyboard.getJSONObject(it) }
                        .firstOrNull { it.optString("button") == target }
                        ?: JSONObject().put("button", target).put("keys", JSONArray()).also {
                            keyboard.put(it)
                        }
                entry.getJSONArray("keys").put(input)
            }
        }
        return true
    }
}
