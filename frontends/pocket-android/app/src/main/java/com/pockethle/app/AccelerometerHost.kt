package com.pockethle.app

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.view.Surface

internal class AccelerometerHost(
    context: Context,
    private val displayRotation: () -> Int,
) : SensorEventListener {
    private val manager = context.getSystemService(SensorManager::class.java)
    private var listening = false

    fun start() {
        if (listening) return
        val sensorManager = manager ?: return
        val sensor = sensorManager.getDefaultSensor(Sensor.TYPE_ACCELEROMETER) ?: return
        listening = sensorManager.registerListener(this, sensor, SensorManager.SENSOR_DELAY_GAME)
        NativeBridge.nativeSetAccelerometer(listening, Float.NaN, Float.NaN, Float.NaN)
    }

    fun stop() {
        if (!listening) return
        listening = false
        manager?.unregisterListener(this)
        NativeBridge.nativeSetAccelerometer(false, 0f, 0f, 0f)
    }

    override fun onSensorChanged(event: SensorEvent) {
        if (!listening || event.sensor.type != Sensor.TYPE_ACCELEROMETER || event.values.size < 3) return
        val (x, y, z) = event.values
        when (displayRotation()) {
            Surface.ROTATION_90 -> NativeBridge.nativeSetAccelerometer(true, -y, x, z)
            Surface.ROTATION_180 -> NativeBridge.nativeSetAccelerometer(true, -x, -y, z)
            Surface.ROTATION_270 -> NativeBridge.nativeSetAccelerometer(true, y, -x, z)
            else -> NativeBridge.nativeSetAccelerometer(true, x, y, z)
        }
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit
}
