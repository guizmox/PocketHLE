package com.pockethle.app

import java.net.InetAddress
import java.net.ServerSocket
import java.io.OutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/** Real HTTP I/O: checkpointing may copy received bytes, never resend a POST. */
class InternetCheckpointTest {
    private class LocalHttp(private val respond: (OutputStream) -> Unit) : AutoCloseable {
        private val listener = ServerSocket(0, 4, InetAddress.getLoopbackAddress())
        val port: Int get() = listener.localPort
        val calls = AtomicInteger()
        init {
            Thread({
                while (!listener.isClosed) {
                    val socket = runCatching { listener.accept() }.getOrNull() ?: break
                    calls.incrementAndGet()
                    Thread({
                        socket.use {
                            it.soTimeout = 10000
                            val reader = it.getInputStream().bufferedReader(Charsets.ISO_8859_1)
                            check(reader.readLine().startsWith("POST / "))
                            var length = 0
                            while (true) {
                                val line = reader.readLine() ?: error("Missing HTTP headers")
                                if (line.isEmpty()) break
                                if (line.startsWith("Content-Length:", true)) length = line.substringAfter(':').trim().toInt()
                            }
                            val body = CharArray(length)
                            var read = 0
                            while (read < length) {
                                val count = reader.read(body, read, length - read)
                                check(count > 0); read += count
                            }
                            check(String(body) == "one POST")
                            respond(it.getOutputStream())
                        }
                    }, "checkpoint-http-response").apply { isDaemon = true; start() }
                }
            }, "checkpoint-http-listener").apply { isDaemon = true; start() }
        }
        override fun close() { listener.close() }
    }
    private fun header(output: OutputStream, status: Int, size: Int) {
        output.write("HTTP/1.1 $status OK\r\nContent-Length: $size\r\nConnection: close\r\n\r\n".toByteArray(Charsets.ISO_8859_1))
    }
    private fun waitUntil(condition: () -> Boolean) {
        val until = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
        while (!condition()) {
            check(System.nanoTime() < until) { "HTTP operation timed out" }
            Thread.sleep(5)
        }
    }
    private fun unpack(packet: ByteArray): Pair<JSONObject, ByteArray> {
        val length = ByteBuffer.wrap(packet).order(ByteOrder.LITTLE_ENDIAN).int
        return JSONObject(String(packet, 4, length, Charsets.UTF_8)) to packet.copyOfRange(4 + length, packet.size)
    }
    private fun start(port: Int): Pair<Int, Int> {
        val client = InternetHost.openSession("""{"access":1,"agent":"checkpoint-test","bypass":"","proxy":""}""")
        val request = InternetHost.start(client,
            """{"server":"127.0.0.1","port":$port,"path":"/","secure":false,"flags":0,"method":"POST","headers":[],"user":"","password":""}""",
            "one POST".toByteArray())
        check(request > 0)
        return client to request
    }
    @Test fun activeCaptureKeepsUnreadSuffixAndLeavesTheOriginalRequestRunning() {
        val release = CountDownLatch(1)
        val server = LocalHttp { output ->
            header(output, 200, 6)
            output.write("abc".toByteArray()); output.flush()
            check(release.await(10, TimeUnit.SECONDS))
            output.write("def".toByteArray())
        }
        val (client, request) = start(server.port)
        try {
            waitUntil { InternetHost.available(request) and 0xffffffffL == 3L }
            assertArrayEquals("a".toByteArray(), InternetHost.read(request, 1).copyOfRange(4, 5))
            val (head, unread) = unpack(InternetHost.checkpoint(request))
            assertEquals(200, head.getJSONObject("head").getInt("status"))
            assertEquals(12030, head.getInt("end_error"))
            assertArrayEquals("bc".toByteArray(), unread)
            assertArrayEquals(unread, unpack(InternetHost.checkpoint(request)).second)
            release.countDown()
            waitUntil { InternetHost.available(request) ushr 32 != 0L }
            val live = InternetHost.read(request, 100)
            assertArrayEquals("bcdef".toByteArray(), live.copyOfRange(4, live.size))
            val (finished, empty) = unpack(InternetHost.checkpoint(request))
            assertEquals(0, finished.getInt("end_error")); assertEquals(0, empty.size)
            assertEquals(1, server.calls.get())
        } finally {
            release.countDown(); InternetHost.closeSession(client); server.close()
        }
    }
    @Test fun captureBeforeHeadersDoesNotInventAResponseOrCancelTheRequest() {
        val release = CountDownLatch(1)
        val entered = CountDownLatch(1)
        val server = LocalHttp { output ->
            entered.countDown()
            check(release.await(10, TimeUnit.SECONDS))
            header(output, 201, 1)
            output.write(byteArrayOf(42))
        }
        val (client, request) = start(server.port)
        try {
            assertTrue(entered.await(10, TimeUnit.SECONDS))
            val (head, bytes) = unpack(InternetHost.checkpoint(request))
            assertTrue(head.isNull("head")); assertEquals(12030, head.getInt("head_error"))
            assertEquals(0, bytes.size)
            release.countDown()
            waitUntil { InternetHost.available(request) ushr 32 != 0L }
            assertEquals(201, JSONObject(InternetHost.head(request)).getInt("status"))
            assertArrayEquals(byteArrayOf(42), InternetHost.read(request, 1).copyOfRange(4, 5))
        } finally {
            release.countDown(); InternetHost.closeSession(client); server.close()
        }
    }
}
