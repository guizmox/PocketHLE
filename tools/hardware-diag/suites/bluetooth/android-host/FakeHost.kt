package com.pockethle.app
import android.bluetooth.BluetoothAdapter
object BluetoothHost {
    private val radio = BluetoothAdapter()
    var discoverableRequests = 0
    fun adapter() = radio
    fun requestDiscoverable() { discoverableRequests++ }
}
