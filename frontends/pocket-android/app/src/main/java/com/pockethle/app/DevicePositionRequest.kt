package com.pockethle.app

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.core.content.ContextCompat

/** One settings action, canceled when its screen closes. No game GPS session needed. */
class DevicePositionRequest(
    private val context: Context,
    private val result: (Location?, String?) -> Unit
) : AutoCloseable, LocationListener {
    private val manager = context.getSystemService(Context.LOCATION_SERVICE) as? LocationManager
    private val handler = Handler(Looper.getMainLooper())
    private var finished = false
    private val timeout = Runnable { complete(null, "No position yet. Enable location and try again outdoors.") }

    @Suppress("MissingPermission")
    fun start() {
        if (finished) return
        val fine = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED
        val coarse = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED
        if (!fine && !coarse) { complete(null, "Location permission denied"); return }
        val m = manager ?: run { complete(null, "Location is unavailable on this device"); return }
        val providers = listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER, LocationManager.PASSIVE_PROVIDER)
            .filter { (fine || it != LocationManager.GPS_PROVIDER) && m.allProviders.contains(it) }
        // A cached fix is useful even if a provider is currently switched off.
        val cached = providers.mapNotNull { runCatching { m.getLastKnownLocation(it) }.getOrNull() }
            .filter { valid(it) }.maxByOrNull { it.elapsedRealtimeNanos }
        if (cached != null) { complete(cached, null); return }
        var subscribed = false
        providers.filter { it != LocationManager.PASSIVE_PROVIDER }.forEach { provider ->
            if (runCatching { m.isProviderEnabled(provider) }.getOrDefault(false)) {
                if (runCatching { m.requestLocationUpdates(provider, 0L, 0f, this, Looper.getMainLooper()) }.isSuccess) subscribed = true
            }
        }
        if (!subscribed) complete(null, "Enable device location and try again")
        else handler.postDelayed(timeout, 20_000L)
    }

    private fun valid(location: Location) = location.latitude.isFinite() && location.longitude.isFinite() &&
        location.latitude in -90.0..90.0 && location.longitude in -180.0..180.0

    override fun onLocationChanged(location: Location) {
        if (valid(location)) complete(location, null)
    }

    private fun complete(location: Location?, message: String?) {
        if (finished) return
        close()
        result(location?.let { Location(it) }, message)
    }

    override fun close() {
        finished = true
        handler.removeCallbacks(timeout)
        runCatching { manager?.removeUpdates(this) }
    }

    override fun onProviderEnabled(provider: String) {}
    override fun onProviderDisabled(provider: String) {}
    @Deprecated("Legacy LocationListener callback")
    override fun onStatusChanged(provider: String?, status: Int, extras: Bundle?) {}
}
