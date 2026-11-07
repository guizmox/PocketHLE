package com.pockethle.app

import org.json.JSONArray
import org.json.JSONObject

/** Edits the existing shared schema; a draft is persisted only by Save. */
internal class InputEditorState(raw: String) {
    val json = JSONObject(raw)
    var keyboard =
        json.optJSONArray("keybindings")
            ?: InputBindings.defaultKeyboard().also { json.put("keybindings", it) }
    var controller =
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
            if (gizmondo) !it.startsWith("pocket_pc_app") && it != "turbo"
            else !it.startsWith("giz_piano")
        }

    fun targets(pad: Boolean, input: String?, long: Boolean, gizmondo: Boolean): List<String?> =
        listOf<String?>(null) +
            visibleButtons(gizmondo) +
            if (
                !gizmondo &&
                    !long &&
                    (!pad || input?.let { DeckInputPolicy.guest(it, false) } == null)
            )
                (0..3).map { "stylus:$it" }
            else emptyList()

    fun label(target: String?, gizmondo: Boolean = false): String =
        when {
                gizmondo && target in gizmondoNames -> gizmondoNames.getValue(target!!)
                !gizmondo && target in pocketNames -> pocketNames.getValue(target!!)
                target == null -> "Unassigned"
                target == "host" -> "Reserved"
                target.startsWith("stylus:") ->
                    "Stylus ${StylusBindings.directions[target.substringAfter(':').toInt()]}"
                else -> InputBindings.buttons[target]?.first ?: target
            }
            .replace("↑", "Up")
            .replace("↓", "Down")
            .replace("←", "Left")
            .replace("→", "Right")

    private val gizmondoNames =
        mapOf(
            "action" to "Play",
            "button_a" to "Stop",
            "button_b" to "Forward",
            "button_c" to "Rewind",
            "soft1" to "L1",
            "soft2" to "R1",
        )

    private val pocketNames =
        mapOf(
            "action" to "ACTION",
            "soft1" to "TAB",
            "soft2" to "ESC",
            "turbo" to "TURBO",
            "button_a" to "A",
            "button_b" to "B",
            "button_c" to "C",
            "pocket_pc_app1" to "P1",
            "pocket_pc_app2" to "P2",
            "pocket_pc_app3" to "P3",
            "pocket_pc_app4" to "P4",
        )

    fun short(target: String?, gizmondo: Boolean = false) =
        when {
            gizmondo && target in gizmondoNames -> gizmondoNames.getValue(target!!)
            target == "action" -> "Act"
            !gizmondo && target == "soft1" -> "Tab"
            !gizmondo && target == "soft2" -> "Esc"
            !gizmondo && target in pocketNames -> pocketNames.getValue(target!!)
            target == null -> "—"
            target == "host" -> "Reserved"
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
        if (fixed(input, pad, gizmondo)) return "host"
        val customKey = InputGestures.bindingKey(gizmondo, pad, input)
        if (shortPress.has(customKey))
            return displayGuest(
                shortPress.optString(customKey).takeIf { it.isNotEmpty() && it != "null" },
                gizmondo,
            )
        if (
            (!pad && DeckInputPolicy.reservedKey(input)) ||
                (pad && DeckInputPolicy.reserved(input, gizmondo))
        )
            return "host"
        if (pad)
            DeckInputPolicy.guest(input, gizmondo)?.let {
                return it
            }
        if (
            !gizmondo &&
                stylus.optBoolean("enabled", true) &&
                longTarget(input, pad, gizmondo) == null
        ) {
            val directions = stylus.optJSONArray(if (pad) "controller" else "keyboard")
            for (i in 0..3) {
                val keys = directions?.optJSONArray(i) ?: continue
                if ((0 until keys.length()).any { same(keys.getString(it), input, pad) })
                    return "stylus:$i"
            }
        }
        if (pad)
            return displayGuest(
                InputBindings.controllerAliases(input).firstNotNullOfOrNull {
                    controller.optString(it).takeIf(String::isNotEmpty)
                },
                gizmondo,
            )
        for (i in 0 until keyboard.length()) {
            val binding = keyboard.getJSONObject(i)
            val keys = binding.optJSONArray("keys") ?: continue
            if ((0 until keys.length()).any { same(keys.getString(it), input, false) })
                return DeckInputPolicy.keyboardGuest(input, binding.getString("button"), gizmondo)
        }
        return if (pad) null else DeckInputPolicy.keyboardGuest(input, null, gizmondo)
    }

    fun shortFor(input: String, pad: Boolean, gizmondo: Boolean): String {
        if (!pad && input.equals("F8", true)) return "Library"
        val target = assignment(input, pad, gizmondo)
        if (target != "host") return short(target, gizmondo)
        val action =
            if (pad) DeckInputPolicy.tap(input) ?: DeckInputPolicy.hold(input, gizmondo)
            else DeckInputPolicy.key(input)
        return action?.let { DeckInputPolicy.short(it) } ?: "Reserved"
    }

    fun assigned(target: String, pad: Boolean, gizmondo: Boolean, inputs: List<String>) =
        inputs.any {
            assignment(it, pad, gizmondo) == target || longTarget(it, pad, gizmondo) == target
        } ||
            longPress.keys().asSequence().any {
                it.startsWith(InputGestures.bindingKey(gizmondo, pad, "")) &&
                    longPress.optString(it) == target &&
                    (if (pad) !DeckInputPolicy.reserved(it.substringAfter(":controller:"), gizmondo)
                    else !DeckInputPolicy.reservedKey(it.substringAfter(":keyboard:")))
            }

    val shortPress =
        json.optJSONObject("short_press_bindings")
            ?: JSONObject().also { json.put("short_press_bindings", it) }
    val longPress =
        json.optJSONObject("long_press_bindings")
            ?: JSONObject().also { json.put("long_press_bindings", it) }

    fun longTarget(input: String, pad: Boolean, gizmondo: Boolean): String? =
        if (fixed(input, pad, gizmondo)) null
        else
            longPress.optString(InputGestures.bindingKey(gizmondo, pad, input)).takeIf {
                it.isNotEmpty()
            }

    fun assignLong(input: String, pad: Boolean, target: String?, gizmondo: Boolean): Boolean {
        if (fixed(input, pad, gizmondo) || (!pad && input == "Escape")) return false
        val key = InputGestures.bindingKey(gizmondo, pad, input)
        if (target == null) longPress.remove(key)
        else if (target in visibleButtons(gizmondo)) {
            if (pad && same(json.optString("gamepad_fullscreen"), input, true))
                json.put("gamepad_fullscreen", JSONObject.NULL)
            longPress.put(key, target)
        } else return false
        return true
    }

    fun reset(pad: Boolean, defaults: JSONObject) {
        shortPress
            .keys()
            .asSequence()
            .toList()
            .filter { it.contains(if (pad) ":controller:" else ":keyboard:") }
            .forEach { shortPress.remove(it) }
        longPress
            .keys()
            .asSequence()
            .toList()
            .filter { it.contains(if (pad) ":controller:" else ":keyboard:") }
            .forEach { longPress.remove(it) }
        if (pad) {
            controller = JSONObject(defaults.getJSONObject("controller").toString())
            json.put("gamepad_bindings", controller)
            json.put("gamepad_fullscreen", defaults.getString("fullscreen"))
        } else {
            keyboard = JSONArray(defaults.getJSONArray("keyboard").toString())
            json.put("keybindings", keyboard)
        }
    }

    private fun displayGuest(target: String?, gizmondo: Boolean) =
        if (gizmondo && target == "turbo") "giz_piano3" else target

    fun fixed(input: String, pad: Boolean, gizmondo: Boolean) =
        if (pad) DeckInputPolicy.reserved(input, gizmondo) else DeckInputPolicy.reservedKey(input)

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
        listOf(shortPress, longPress).forEach { table ->
            table.keys().forEach { key ->
                val marker = if (pad) ":controller:" else ":keyboard:"
                val index = key.indexOf(marker)
                if (index >= 0) keys.add(key.substring(index + marker.length))
            }
        }
        return keys.distinct().sorted()
    }

    fun assign(input: String, pad: Boolean, target: String?, gizmondo: Boolean? = null): Boolean {
        if (gizmondo != null && fixed(input, pad, gizmondo)) return false
        if (!pad && DeckInputPolicy.reservedKey(input)) return false
        if (gizmondo != null) {
            val key = InputGestures.bindingKey(gizmondo, pad, input)
            if (target?.startsWith("stylus:") == true) {
                shortPress.remove(key)
                longPress.remove(key)
            } else {
                shortPress.put(key, target ?: JSONObject.NULL)
                if (pad && same(json.optString("gamepad_fullscreen"), input, true))
                    json.put("gamepad_fullscreen", JSONObject.NULL)
                return true
            }
        }
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
