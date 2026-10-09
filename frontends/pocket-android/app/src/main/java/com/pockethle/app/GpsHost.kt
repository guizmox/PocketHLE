package com.pockethle.app

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import androidx.core.content.ContextCompat
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger

/** Foreground-only location subscriptions, latest position only. No Activity retained. */
object GpsHost {
    private lateinit var context: Context
    private val thread = HandlerThread("pockethle-gps").apply { start() }
    private val handler = Handler(thread.looper)
    private val sessions = ConcurrentHashMap<Int, Session>()
    private val ids = AtomicInteger(1)
    @Volatile private var foreground = false
    fun initialize(ctx: Context) { context = ctx.applicationContext }
    fun permissions() = arrayOf(Manifest.permission.ACCESS_COARSE_LOCATION, Manifest.permission.ACCESS_FINE_LOCATION)
    private fun granted(permission: String) = ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED
    fun resume() { foreground = true; handler.post { sessions.values.forEach { it.start() } } }
    fun pause() { foreground = false; handler.post { sessions.values.forEach { it.release() } } }
    fun closeAll() { sessions.keys.toList().forEach { close(it) } }
    fun open(): Int {
        if (!granted(Manifest.permission.ACCESS_COARSE_LOCATION) && !granted(Manifest.permission.ACCESS_FINE_LOCATION))
            throw SecurityException("Location permission denied")
        val id = ids.getAndIncrement()
        if (id <= 0) throw IllegalStateException("Location session limit")
        val s = Session(); sessions[id] = s; handler.post { s.start() }; return id
    }
    fun read(id: Int): ByteArray {
        val s = sessions[id] ?: return error(6)
        if (!foreground) return ByteArray(0)
        if (s.error != 0) return error(s.error)
        return s.packet ?: ByteArray(0)
    }
    fun close(id: Int) { val s = sessions.remove(id) ?: return; s.closed = true; handler.post { s.release() } }
    private fun error(e: Int) = ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(e).array()
    private class Session : LocationListener {
        @Volatile var closed = false
        @Volatile var error = 0
        @Volatile var packet: ByteArray? = null
        private var manager: LocationManager? = null
        private var subscribed = false
        private var lastLocation: Location? = null
        fun release() { runCatching { manager?.removeUpdates(this) }; manager = null; subscribed = false; packet = null; lastLocation = null }
        @Suppress("MissingPermission")
        fun start() {
            if (closed || !foreground || subscribed) return
            error = 0
            try {
                val m = context.getSystemService(Context.LOCATION_SERVICE) as LocationManager
                manager = m
                val fine = granted(Manifest.permission.ACCESS_FINE_LOCATION)
                if (!fine && !granted(Manifest.permission.ACCESS_COARSE_LOCATION)) throw SecurityException()
                val providers = listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER).filter {
                    (fine || it != LocationManager.GPS_PROVIDER) && m.allProviders.contains(it)
                }
                if (providers.isEmpty()) { error = 21; return }
                providers.forEach { m.requestLocationUpdates(it, 1000L, 0f, this, thread.looper) }
                subscribed = true
            } catch (_: SecurityException) { error = 5; release() }
              catch (_: Exception) { error = 21; release() }
        }
        override fun onLocationChanged(location: Location) {
            if (closed || !foreground) return
            val previous = lastLocation
            if (previous != null && previous.hasAccuracy() && location.hasAccuracy() &&
                previous.accuracy < location.accuracy && location.time - previous.time < 10000L) return
            lastLocation = Location(location)
            val b = ByteBuffer.allocate(64).order(ByteOrder.LITTLE_ENDIAN)
            b.putLong(location.time)
            b.putDouble(location.latitude); b.putDouble(location.longitude)
            // Old Android altitude is ellipsoidal, not MSL. API34 exposes actual MSL.
            b.putDouble(if (Build.VERSION.SDK_INT >= 34 && location.hasMslAltitude()) location.mslAltitudeMeters else Double.NaN)
            b.putDouble(if (location.hasSpeed()) location.speed.toDouble() else Double.NaN)
            b.putDouble(if (location.hasBearing()) location.bearing.toDouble() else Double.NaN)
            b.putDouble(if (location.hasAccuracy()) location.accuracy.toDouble() else 42949672.95)
            b.putDouble(if (Build.VERSION.SDK_INT >= 34 && location.hasMslAltitudeAccuracy()) location.mslAltitudeAccuracyMeters.toDouble() else Double.NaN)
            packet = b.array(); error = 0
        }
        override fun onProviderDisabled(provider: String) { packet = null }
        override fun onProviderEnabled(provider: String) { error = 0 }
        @Deprecated("Legacy LocationListener callback")
        override fun onStatusChanged(provider: String?, status: Int, extras: Bundle?) {}
    }
}
