package com.pockethle.app

import android.annotation.SuppressLint
import android.bluetooth.BluetoothServerSocket
import android.bluetooth.BluetoothSocket
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.LinkedBlockingDeque
import java.util.concurrent.atomic.AtomicInteger

/** Chicane uses AF_BTH Winsock, rather than the SDK's virtual COM port.
 * UUIDs replace physical RFCOMM channel numbers on Android's public API.
 * Every blocking native operation belongs to a cancellable host worker. */
@SuppressLint("MissingPermission")
object RfcommHost {
    private val next = AtomicInteger(1)
    private val sockets = ConcurrentHashMap<Int, Endpoint>()
    private val gate = Any()
    private var enabled = true
    private class Failure(val code: Int) : Exception()
    private fun fail(code: Int): Nothing = throw Failure(code)
    private class Endpoint {
        var bound: JSONObject? = null
        var peer: JSONObject? = null
        @Volatile var socket: BluetoothSocket? = null
        @Volatile var server: BluetoothServerSocket? = null
        @Volatile var closed = false
        @Volatile var connecting = false
        @Volatile var connected = false
        @Volatile var eof = false
        @Volatile var error = 0
        @Volatile var readOff = false
        @Volatile var writeOff = false
        val rx = LinkedBlockingDeque<Byte>(65536)
        val tx = ArrayBlockingQueue<ByteArray>(16)
        val accepted = ArrayBlockingQueue<Endpoint>(8)
        val workers = java.util.concurrent.CopyOnWriteArrayList<Thread>()
        @Synchronized fun publish(s: BluetoothSocket): Boolean {
            if (closed) { runCatching { s.close() }; return false }
            socket = s; return true
        }
        @Synchronized fun publish(s: BluetoothServerSocket): Boolean {
            if (closed) { runCatching { s.close() }; return false }
            server = s; return true
        }
        @Synchronized fun close() {
            if (closed) return
            closed = true
            runCatching { server?.close() }; server = null
            runCatching { socket?.close() }; socket = null
            workers.forEach { it.interrupt() }
            while (true) (accepted.poll() ?: break).close()
            rx.clear(); tx.clear()
        }
        fun worker(name: String, f: () -> Unit) {
            val t = Thread({
                try { f() } catch (_: InterruptedException) {}
                catch (_: SecurityException) { if (!closed) error = 10013 }
                catch (_: Exception) { if (!closed && !(readOff && writeOff)) error = if (connected) 10054 else 10061 }
            }, name)
            synchronized(this) {
                if (closed) return
                workers.add(t); t.start()
            }
        }
        fun pump() {
            connected = true; connecting = false
            worker("PocketHLE-RFCOMM-write") {
                while (!closed) {
                    val bytes = tx.take()
                    socket!!.outputStream.write(bytes)
                }
            }
            val bytes = ByteArray(4096)
            while (!closed) {
                val n = socket!!.inputStream.read(bytes)
                if (n < 0) { eof = true; break }
                for (i in 0 until n) {
                    if (readOff || closed) break
                    rx.putLast(bytes[i])
                }
            }
        }
    }
    fun initialize() { synchronized(gate) { enabled = true } }
    fun closeAll() {
        val old = synchronized(gate) {
            enabled = false
            sockets.values.toList().also { sockets.clear() }
        }
        old.forEach { it.close() }
    }
    private fun add(e: Endpoint): Int = synchronized(gate) {
        if (!enabled) { e.close(); fail(10091) }
        if (sockets.size >= 64) { e.close(); fail(10024) }
        val id = next.getAndIncrement()
        if (id <= 0) { e.close(); fail(10024) }
        sockets[id] = e; id
    }
    private fun address(a: JSONObject): JSONObject = JSONObject()
        .put("address", a.optLong("address", 0)).put("uuid", a.optString("uuid", ""))
        .put("port", a.optLong("port", 0))
    private fun service(a: JSONObject): UUID {
        val port = a.optLong("port", 0)
        if (port !in 0..30 && port != 4294967295L) fail(10022)
        val explicit = a.optString("uuid", "")
        if (explicit.isNotEmpty() && explicit != "00000000-0000-0000-0000-000000000000")
            return UUID.fromString(explicit)
        // Same GUID as PortParams::service_uuid, including SDK channel 2.
        return UUID.fromString("454c4850-5442-4241-9121-47495a0000%02x".format(if (port == 4294967295L || port == 0L) 1 else port))
    }
    private fun mac(a: Long) = (5 downTo 0).joinToString(":") { "%02X".format((a ushr (it * 8)) and 255) }
    private fun peer(s: BluetoothSocket, bound: JSONObject?): JSONObject = address(bound ?: JSONObject())
        .put("address", s.remoteDevice.address.replace(":", "").toLong(16))

    /** Small control messages only. Payload bytes use dedicated JNI array methods. */
    @JvmStatic fun call(request: String): String {
        val out = JSONObject()
        try {
            val r = JSONObject(request)
            val op = r.getString("op")
            if (op == "new") {
                if (!BluetoothHost.adapter().isEnabled) fail(10091)
                return out.put("id", add(Endpoint())).toString()
            }
            val id = r.getInt("id")
            val e = sockets[id] ?: fail(10038)
            synchronized(e) {
                if (e.closed) fail(10038)
                when (op) {
                    "close" -> { sockets.remove(id); e.close() }
                    "bind" -> {
                        if (e.bound != null || e.socket != null || e.server != null) fail(10022)
                        val a = address(r.getJSONObject("addr")); service(a)
                        if (a.getLong("address") != 0L) fail(10049)
                        if (a.getLong("port") == 0L || a.getLong("port") == 4294967295L) a.put("port", 1)
                        e.bound = a
                    }
                    "listen" -> {
                        if (e.server != null) return out.toString()
                        if (e.socket != null) fail(10022)
                        val a = e.bound ?: fail(10022)
                        val server = BluetoothHost.adapter().listenUsingRfcommWithServiceRecord("PocketHLE RFCOMM", service(a))
                        if (!e.publish(server)) fail(10038)
                        val backlog = r.optInt("backlog", 1).coerceIn(1, 8)
                        BluetoothHost.requestDiscoverable()
                        e.worker("PocketHLE-RFCOMM-accept") {
                            while (!e.closed) {
                                val socket = server.accept()
                                val child = Endpoint()
                                if (!child.publish(socket)) continue
                                child.bound = address(a); child.peer = peer(socket, a)
                                child.connected = true
                                synchronized(e) {
                                    if (e.closed || e.accepted.size >= backlog || !e.accepted.offer(child)) child.close()
                                }
                            }
                        }
                    }
                    "connect" -> {
                        if (e.server != null) fail(10022)
                        if (e.connected) fail(10056)
                        if (e.connecting) fail(10037)
                        val a = address(r.getJSONObject("addr"))
                        val remote = a.getLong("address")
                        if (remote == 0L || remote ushr 48 != 0L) fail(10049)
                        val socket = BluetoothHost.adapter().getRemoteDevice(mac(remote)).createRfcommSocketToServiceRecord(service(a))
                        if (!e.publish(socket)) fail(10038)
                        e.peer = a; e.connecting = true
                        e.worker("PocketHLE-RFCOMM-connect-read") {
                            BluetoothHost.adapter().cancelDiscovery()
                            socket.connect()
                            if (!e.closed) e.pump()
                        }
                        fail(10035)
                    }
                    "accept" -> {
                        if (e.server == null) fail(10022)
                        if (e.error != 0) fail(e.error)
                        val child = e.accepted.poll() ?: fail(10035)
                        val acceptedId = add(child)
                        child.worker("PocketHLE-RFCOMM-read") { child.pump() }
                        out.put("id", acceptedId).put("addr", child.peer)
                    }
                    "ready" -> {
                        out.put("read", e.accepted.isNotEmpty() || e.rx.isNotEmpty() || e.eof || e.readOff)
                            .put("write", e.connected && !e.writeOff && e.error == 0 && e.tx.remainingCapacity() > 0)
                            .put("error", e.error != 0).put("code", e.error)
                    }
                    "available" -> out.put("count", e.rx.size)
                    "address" -> {
                        val a = if (r.optBoolean("peer")) {
                            if (!e.connected) fail(10057)
                            e.peer ?: fail(10057)
                        } else e.bound ?: address(JSONObject())
                        out.put("addr", a)
                    }
                    "shutdown" -> {
                        if (!e.connected) fail(10057)
                        val how = r.getInt("how")
                        if (how !in 0..2) fail(10022)
                        if (how != 1) { e.readOff = true; e.rx.clear() }
                        if (how != 0) e.writeOff = true
                        // BluetoothSocket has no native half-close. Full shutdown releases it.
                        if (how == 2) {
                            runCatching { e.socket?.close() }; e.eof = true
                            e.tx.clear(); e.workers.forEach { it.interrupt() }
                        }
                        Unit
                    }
                    "error" -> out.put("code", e.error)
                    else -> fail(10045)
                }
            }
        } catch (e: Failure) { out.put("error_code", e.code) }
        catch (_: SecurityException) { out.put("error_code", 10013) }
        catch (_: IllegalArgumentException) { out.put("error_code", 10022) }
        catch (_: Exception) { out.put("error_code", 10091) }
        return out.toString()
    }
    @JvmStatic fun read(id: Int, count: Int, peek: Boolean): ByteArray {
        val e = sockets[id] ?: return byteArrayOf()
        synchronized(e) {
            val bytes = ByteArray(count.coerceIn(0, 65536).coerceAtMost(e.rx.size))
            if (peek) {
                val it = e.rx.iterator()
                for (i in bytes.indices) { if (!it.hasNext()) return bytes.copyOf(i); bytes[i] = it.next() }
            } else for (i in bytes.indices) bytes[i] = e.rx.pollFirst() ?: return bytes.copyOf(i)
            return bytes
        }
    }
    @JvmStatic fun status(id: Int): Int {
        val e = sockets[id] ?: return 10038
        if (e.closed) return 10038
        if (e.rx.isNotEmpty()) return 0
        if (e.readOff || e.eof) return -1
        if (e.error != 0) return e.error
        if (!e.connected) return if (e.connecting) 10035 else 10057
        return 0
    }
    @JvmStatic fun write(id: Int, bytes: ByteArray): Int {
        val e = sockets[id] ?: return -10038
        synchronized(e) {
            if (e.closed) return -10038
            if (e.writeOff) return -10058
            if (e.error != 0) return -e.error
            if (!e.connected) return if (e.connecting) -10035 else -10057
            val n = minOf(bytes.size, 4096)
            return if (n == 0) 0 else if (e.tx.offer(bytes.copyOf(n))) n else -10035
        }
    }
}
