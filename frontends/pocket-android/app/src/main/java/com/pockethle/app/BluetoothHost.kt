package com.pockethle.app

import android.app.Activity
import android.bluetooth.BluetoothAdapter
import java.lang.ref.WeakReference
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import android.Manifest
import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothServerSocket
import android.bluetooth.BluetoothSocket
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.LinkedBlockingDeque
import java.util.concurrent.atomic.AtomicInteger

/** All blocking Bluetooth operations live on host threads, never the emulator. */
@SuppressLint("MissingPermission")
object BluetoothHost {
    private var context: Context? = null
    private var activity = WeakReference<Activity>(null)
    @Volatile private var lastDiscoverableRequest = 0L
    @Volatile private var active = true
    private val scanGate = Any()
    @Volatile private var scanFinished: CountDownLatch? = null
    private val next = AtomicInteger(1)
    private val connections = ConcurrentHashMap<Int, Connection>()
    @Synchronized fun initialize(context: Context) {
        this.context = context.applicationContext
        activity = WeakReference(context as? Activity)
        lastDiscoverableRequest = 0L
        active = true
        RfcommHost.initialize()
    }
    fun requestDiscoverable() {
        val owner = activity.get() ?: return
        owner.runOnUiThread {
            if (owner.isFinishing || owner.isDestroyed || activity.get() !== owner ||
                (lastDiscoverableRequest != 0L && System.nanoTime() - lastDiscoverableRequest < 15_000_000_000L)) return@runOnUiThread
            runCatching {
                if (adapter().scanMode == BluetoothAdapter.SCAN_MODE_CONNECTABLE_DISCOVERABLE) return@runCatching
                lastDiscoverableRequest = System.nanoTime()
                owner.startActivity(Intent(BluetoothAdapter.ACTION_REQUEST_DISCOVERABLE)
                    .putExtra(BluetoothAdapter.EXTRA_DISCOVERABLE_DURATION, 300))
            }.onFailure { lastDiscoverableRequest = 0L }
        }
    }
    @Synchronized fun closeAll() {
        active = false
        scanFinished?.countDown()
        runCatching { adapter().cancelDiscovery() }
        activity.clear()
        RfcommHost.closeAll()
        connections.keys.toList().forEach { close(it) }
    }
    fun permissions(): Array<String> = if (Build.VERSION.SDK_INT >= 31)
        arrayOf(Manifest.permission.BLUETOOTH_CONNECT, Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_ADVERTISE)
    else arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)
    internal fun adapter() = (context?.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager)?.adapter
        ?: throw IllegalStateException("No Bluetooth adapter")
    @JvmStatic fun ready(): Int = if (active && adapter().isEnabled) 0 else 10091
    @JvmStatic fun hostname(): String = adapter().name ?: "PocketHLE Android"

    @JvmStatic fun scan(): String = synchronized(scanGate) {
        val ctx = context ?: throw IllegalStateException("Bluetooth not initialized")
        val adapter = adapter()
        if (!adapter.isEnabled) throw IllegalStateException("Bluetooth disabled")
        val devices = ConcurrentHashMap<String, BluetoothDevice>()
        adapter.bondedDevices.forEach { devices[it.address] = it }
        val finished = CountDownLatch(1)
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                if (intent.action == BluetoothAdapter.ACTION_DISCOVERY_FINISHED) { finished.countDown(); return }
                @Suppress("DEPRECATION")
                val device = intent.getParcelableExtra<BluetoothDevice>(BluetoothDevice.EXTRA_DEVICE)
                if (device != null) devices[device.address] = device
            }
        }
        val filter = IntentFilter(BluetoothDevice.ACTION_FOUND).apply { addAction(BluetoothAdapter.ACTION_DISCOVERY_FINISHED) }
        // Bluetooth broadcasts come from another privileged system process.
        if (Build.VERSION.SDK_INT >= 33) ctx.registerReceiver(receiver, filter, Context.RECEIVER_EXPORTED)
        else ctx.registerReceiver(receiver, filter)
        try {
            synchronized(this) {
                if (!active) throw IllegalStateException("Bluetooth session closed")
                scanFinished = finished
                if (!adapter.startDiscovery()) throw IllegalStateException("Cannot start Bluetooth discovery")
            }
            // startDiscovery is asynchronous: isDiscovering can still be false here.
            finished.await(15, TimeUnit.SECONDS)
        } finally {
            scanFinished = null
            runCatching { adapter.cancelDiscovery() }
            ctx.unregisterReceiver(receiver)
        }
        JSONArray().apply {
            devices.values.sortedBy { it.address }.forEach {
                put(JSONObject().put("address", it.address.replace(":", "").toLong(16)).put("name", it.name ?: "<unnamed>"))
            }
        }.toString()
    }

    private class Connection {
        @Volatile var socket: BluetoothSocket? = null
        @Volatile var server: BluetoothServerSocket? = null
        @Volatile var error = 0
        @Volatile var connected = false
        @Volatile var eof = false
        @Volatile var closed = false
        val rx = LinkedBlockingDeque<Byte>(65536)
        val tx = ArrayBlockingQueue<ByteArray>(128)
        val threads = java.util.concurrent.CopyOnWriteArrayList<Thread>()
        fun close() {
            closed = true; error = 995
            runCatching { server?.close() }; runCatching { socket?.close() }
            threads.forEach { it.interrupt() }; rx.clear(); tx.clear()
        }
    }
    @Synchronized @JvmStatic fun open(server: Boolean, address: Long, uuid: String): Int {
        val adapter = adapter()
        if (!adapter.isEnabled) return -10091
        if (!active) return -10091
        if (connections.size >= 64) return -10024
        val connection = Connection()
        val id = next.getAndIncrement()
        if (id <= 0) return -8
        val service = UUID.fromString(uuid)
        // Create sockets synchronously so permission and registration failures
        // return immediately. Blocking connect/accept starts on the worker.
        if (server) {
            connection.server = adapter.listenUsingRfcommWithServiceRecord("PocketHLE RFCOMM", service)
            requestDiscoverable()
        }
        else {
            val mac = (5 downTo 0).joinToString(":") { "%02X".format((address ushr (it * 8)) and 255) }
            connection.socket = adapter.getRemoteDevice(mac).createRfcommSocketToServiceRecord(service)
        }
        connections[id] = connection
        val reader = Thread({
            try {
                adapter.cancelDiscovery()
                if (server) {
                    val socket = connection.server!!.accept()
                    connection.socket = socket
                    connection.server?.close(); connection.server = null
                    if (connection.closed) { socket.close(); return@Thread }
                } else connection.socket!!.connect()
                if (connection.closed) return@Thread
                connection.connected = true
                val writer = Thread({
                    try {
                        while (!connection.closed) {
                            val bytes = connection.tx.take()
                            connection.socket!!.outputStream.write(bytes)
                        }
                    } catch (_: InterruptedException) {} catch (_: Exception) { connection.error = 10054 }
                }, "PocketHLE-BT-write")
                connection.threads.add(writer); writer.start()
                val buffer = ByteArray(4096)
                while (!connection.closed) {
                    val n = connection.socket!!.inputStream.read(buffer)
                    if (n < 0) { connection.eof = true; break }
                    for (i in 0 until n) connection.rx.putLast(buffer[i])
                }
            } catch (_: InterruptedException) {} catch (_: SecurityException) { connection.error = 10013 }
            catch (_: Exception) { if (!connection.closed) connection.error = 10054 }
        }, "PocketHLE-BT-connect-read")
        connection.threads.add(reader); reader.start()
        return id
    }
    @JvmStatic fun status(id: Int): Int {
        val c = connections[id] ?: return 995
        if (c.closed) return 995
        if (c.rx.isNotEmpty()) return 0
        if (c.error != 0) return c.error
        if (!c.connected) return 10035
        if (c.eof) return -1
        return 0
    }
    @JvmStatic fun read(id: Int, count: Int): ByteArray {
        val c = connections[id] ?: return byteArrayOf()
        val bytes = ByteArray(minOf(count, c.rx.size))
        for (i in bytes.indices) bytes[i] = c.rx.pollFirst() ?: return bytes.copyOf(i)
        return bytes
    }
    @JvmStatic fun write(id: Int, bytes: ByteArray): Int {
        val c = connections[id] ?: return -995
        if (c.closed || c.error != 0) return -(if (c.closed) 995 else c.error)
        if (!c.connected) return -10035
        if (c.eof) return -10054
        if (bytes.isEmpty()) return 0
        val n = minOf(bytes.size, 4096)
        return if (c.tx.offer(bytes.copyOf(n))) n else -10035
    }
    @JvmStatic fun close(id: Int) { connections.remove(id)?.close() }
}
