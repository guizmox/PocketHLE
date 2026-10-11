package android.bluetooth

import java.io.InputStream
import java.io.OutputStream
import java.io.PipedInputStream
import java.io.PipedOutputStream
import java.io.IOException
import java.util.UUID
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger

// JVM test doubles only; no Android or physical radio emulation in production.
class BluetoothDevice(val address: String) {
    fun createRfcommSocketToServiceRecord(uuid: UUID) = BluetoothSocket(this, uuid)
}
class BluetoothSocket(val remoteDevice: BluetoothDevice, private val uuid: UUID? = null) {
    private val input = PipedInputStream(65536)
    private val output = PipedOutputStream()
    val inputStream: InputStream get() = input
    val outputStream: OutputStream get() = object : OutputStream() {
        override fun write(b: Int) { output.write(b); output.flush() }
        override fun write(b: ByteArray, offset: Int, length: Int) { output.write(b, offset, length); output.flush() }
    }
    @Volatile private var closed = false
    init { BluetoothAdapter.live.incrementAndGet() }
    fun connect() {
        if (BluetoothAdapter.connectDelayMs > 0) Thread.sleep(BluetoothAdapter.connectDelayMs)
        val server = BluetoothAdapter.servers[uuid] ?: throw IOException("No server")
        val peer = BluetoothSocket(BluetoothDevice("12:34:56:78:9A:BC"))
        output.connect(peer.input); peer.output.connect(input)
        server.pending.put(peer)
    }
    @Synchronized fun close() {
        if (closed) return
        closed = true; BluetoothAdapter.live.decrementAndGet()
        runCatching { input.close() }; runCatching { output.close() }
    }
}
class BluetoothServerSocket(private val uuid: UUID) {
    val pending = LinkedBlockingQueue<BluetoothSocket>()
    @Volatile private var closed = false
    init { BluetoothAdapter.live.incrementAndGet() }
    fun accept(): BluetoothSocket {
        while (!closed) {
            pending.poll(10, java.util.concurrent.TimeUnit.MILLISECONDS)?.let { return it }
        }
        throw IOException("Server closed")
    }
    @Synchronized fun close() {
        if (closed) return
        closed = true; BluetoothAdapter.live.decrementAndGet()
        BluetoothAdapter.servers.remove(uuid, this)
        while (true) (pending.poll() ?: break).close()
    }
}
class BluetoothAdapter {
    companion object {
        val servers = ConcurrentHashMap<UUID, BluetoothServerSocket>()
        val live = AtomicInteger()
        @Volatile var connectDelayMs = 0L
    }
    var isEnabled = true
    fun cancelDiscovery() = true
    fun getRemoteDevice(mac: String) = BluetoothDevice(mac)
    fun listenUsingRfcommWithServiceRecord(name: String, uuid: UUID): BluetoothServerSocket {
        check(name.isNotEmpty())
        val s = BluetoothServerSocket(uuid)
        if (servers.putIfAbsent(uuid, s) != null) { s.close(); throw IOException("In use") }
        return s
    }
}
