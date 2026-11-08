package com.pockethle.app

import android.view.InputDevice
import android.view.KeyEvent

/** Presentation only. Never infer markings from a device name/vendor or change input IDs. */
internal object ControllerLabels {
    private var activeDevice: Int? = null
    var revision: Int = 0
        private set

    fun observe(event: KeyEvent) {
        val sources = event.device?.sources ?: event.source
        if (event.action == KeyEvent.ACTION_DOWN &&
            (sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD ||
             sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK)) {
            if (activeDevice != event.deviceId) {
                activeDevice = event.deviceId
                revision++
            }
        }
    }

    fun grid(input: GridInput): String = when {
        input.source == "keyboard" -> "Key ${input.name}"
        input.name.startsWith("Touch:") -> "Touch ${input.name.removePrefix("Touch:")}"
        else -> label(input.name)
    }

    fun text(template: String): String {
        var result=template
        for(control in listOf("South","East","West","North")) {
            if(result.contains("{$control}")) result=result.replace("{$control}",label(control))
        }
        return result
    }

    fun label(control: String, other: String = control): String {
        val key = when (control) {
            "South" -> KeyEvent.KEYCODE_BUTTON_A
            "East" -> KeyEvent.KEYCODE_BUTTON_B
            "West" -> KeyEvent.KEYCODE_BUTTON_X
            "North" -> KeyEvent.KEYCODE_BUTTON_Y
            else -> return other
        }
        val device = activeDevice?.let { InputDevice.getDevice(it) }
            ?: InputDevice.getDeviceIds().toList().mapNotNull { InputDevice.getDevice(it) }.filter {
                it.sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD ||
                    it.sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
            }.singleOrNull()
        // Android often provides no display character for controller buttons.
        // Do not substitute the normalized KEYCODE_BUTTON_A name as a marking.
        return reportedControllerLabel(control, device?.keyCharacterMap?.getDisplayLabel(key))
    }
}

internal fun reportedControllerLabel(control: String, reported: Char?): String =
    if (reported != null && !reported.isISOControl() && !reported.isWhitespace()) reported.toString()
    else when (control) {
        "South" -> "Bottom button"
        "East" -> "Right button"
        "West" -> "Left button"
        "North" -> "Top button"
        else -> control
    }
