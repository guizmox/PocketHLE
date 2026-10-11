package com.pockethle.app

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.provider.Settings
import android.telephony.TelephonyManager

/** Read-only snapshots. No listener, Activity reference, timer or device identifier. */
object DeviceStatusHost {
    @Volatile private var context: Context? = null
    fun initialize(ctx: Context) { context = ctx.applicationContext }
    fun permissions(): Array<String> {
        val ctx = context ?: return emptyArray()
        if (Build.VERSION.SDK_INT < 26 || !ctx.packageManager.hasSystemFeature(PackageManager.FEATURE_TELEPHONY)) return emptyArray()
        return arrayOf(Manifest.permission.READ_PHONE_STATE, Manifest.permission.ACCESS_COARSE_LOCATION)
    }
    @JvmStatic fun read(): Int {
        val ctx = context ?: return 255 shl 16
        val airplane = try {
            when (Settings.Global.getInt(ctx.contentResolver, Settings.Global.AIRPLANE_MODE_ON, -1)) {
                0 -> 1
                1 -> 2
                else -> 0
            }
        } catch (_: RuntimeException) { 0 }
        val sim: Int
        var cellular = 255
        if (ctx.packageManager.hasSystemFeature(PackageManager.FEATURE_TELEPHONY)) {
            val phone = ctx.getSystemService(Context.TELEPHONY_SERVICE) as? TelephonyManager
            sim = try { phone?.simState ?: 0 } catch (_: RuntimeException) { 0 }
            if (Build.VERSION.SDK_INT >= 26) {
                cellular = try { phone?.serviceState?.state ?: 255 } catch (_: RuntimeException) { 255 }
            }
        } else { sim = 1; cellular = 3 }
        // Dummy ActiveSync stays disconnected; Wi-Fi/HTTP is projected separately.
        return airplane or ((sim and 255) shl 8) or ((cellular and 255) shl 16)
    }
}
