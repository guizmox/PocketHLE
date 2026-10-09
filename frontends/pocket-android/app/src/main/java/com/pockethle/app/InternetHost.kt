package com.pockethle.app

import org.json.JSONArray
import org.json.JSONObject
import java.net.*
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.*
import java.util.concurrent.atomic.AtomicInteger
import javax.net.ssl.SSLException

/** Per-session cookies, verified HTTPS, bounded response streaming. No global
 * CookieHandler, trust manager, hostname verifier or application Context changes. */
object InternetHost {
    private val ids = AtomicInteger(1)
    private val clients = ConcurrentHashMap<Int, Client>()
    private val requests = ConcurrentHashMap<Int, Request>()
    private val closer = ThreadPoolExecutor(1, 2, 10L, TimeUnit.SECONDS, ArrayBlockingQueue<Runnable>(128),
        ThreadFactory { task -> Thread(task, "pockethle-http-close").apply { isDaemon = true } }, ThreadPoolExecutor.DiscardPolicy())
    private class Client(val spec: JSONObject) {
        val cookies = CookieManager(null, CookiePolicy.ACCEPT_ORIGINAL_SERVER)
        @Volatile var closed = false
    }
    private class Request(val parent: Int, val client: Client) {
        val lock = Object()
        val bytes = ByteArray(65536)
        var offset = 0; var count = 0
        @Volatile var closed = false
        @Volatile var head = ""
        @Volatile var error = 0
        @Volatile var done = false
        @Volatile var connection: HttpURLConnection? = null
        @Volatile var worker: Thread? = null
        fun cancel() {
            closed = true
            synchronized(lock) { count = 0; lock.notifyAll() }
            worker?.interrupt()
            val current = connection
            if (current != null) closer.execute { runCatching { current.disconnect() } }
        }
        fun push(source: ByteArray, length: Int) {
            var start = 0
            synchronized(lock) {
                while (start < length) {
                    while (count == bytes.size && !closed) lock.wait()
                    if (closed) throw InterruptedException()
                    val n = minOf(length - start, bytes.size - count)
                    val end = (offset + count) % bytes.size
                    val first = minOf(n, bytes.size - end)
                    System.arraycopy(source, start, bytes, end, first)
                    System.arraycopy(source, start + first, bytes, 0, n - first)
                    count += n; start += n; lock.notifyAll()
                }
            }
        }
    }
    fun openSession(json: String): Int {
        val id = ids.getAndIncrement(); if (id <= 0) return -8
        clients[id] = Client(JSONObject(json)); return id
    }
    fun closeSession(id: Int) {
        val client = clients.remove(id) ?: return
        client.closed = true
        requests.filterValues { it.parent == id }.keys.toList().forEach { close(it) }
        client.cookies.cookieStore.removeAll()
    }
    fun closeAll() { clients.keys.toList().forEach { closeSession(it) } }
    fun start(parent: Int, json: String, body: ByteArray): Int {
        val client = clients[parent] ?: return -6
        if (client.closed) return -6
        val id = ids.getAndIncrement(); if (id <= 0) return -8
        val spec = JSONObject(json)
        val request = Request(parent, client); requests[id] = request
        val worker = Thread({
            try { execute(request, spec, body) }
            catch (_: InterruptedException) { request.error = 12017 }
            catch (_: UnknownHostException) { request.error = 12007 }
            catch (_: SocketTimeoutException) { request.error = 12002 }
            catch (_: ConnectException) { request.error = 12029 }
            catch (_: SSLException) { request.error = 12045 }
            catch (_: ProtocolException) { request.error = 87 }
            catch (_: IllegalArgumentException) { request.error = 87 }
            catch (_: Exception) { request.error = if (request.closed) 12017 else 12030 }
            finally {
                request.done = true
                runCatching { request.connection?.disconnect() }; request.connection = null
                synchronized(request.lock) { request.lock.notifyAll() }
            }
        }, "pockethle-http").apply { isDaemon = true }
        request.worker = worker
        try { worker.start() } catch (_: Exception) { requests.remove(id); request.cancel(); return -8 }
        if (client.closed) close(id)
        return id
    }
    fun head(id: Int): String {
        val request = requests[id] ?: return "{\"error\":6}"
        if (request.head.isNotEmpty()) return request.head
        return if (request.error != 0) JSONObject().put("error", request.error).toString() else ""
    }
    fun available(id: Int): Long {
        val r = requests[id] ?: return -6
        synchronized(r.lock) {
            if (r.closed) return -12017
            if (r.count == 0 && r.error != 0) return -r.error.toLong()
            return r.count.toLong() or (if (r.done) 1L shl 32 else 0L)
        }
    }
    fun read(id: Int, size: Int): ByteArray {
        val r = requests[id] ?: return packet(-6)
        synchronized(r.lock) {
            if (r.closed) return packet(-12017)
            val n = minOf(maxOf(size, 0), r.count, 65536)
            if (n == 0) return packet(if (r.error != 0) -r.error else if (r.done || size == 0) 1 else 0)
            val out = ByteArray(4 + n)
            val first = minOf(n, r.bytes.size - r.offset)
            System.arraycopy(r.bytes, r.offset, out, 4, first)
            System.arraycopy(r.bytes, 0, out, 4 + first, n - first)
            r.offset = (r.offset + n) % r.bytes.size; r.count -= n; r.lock.notifyAll()
            return out
        }
    }
    private fun packet(status: Int) = ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(status).array()
    fun close(id: Int) { requests.remove(id)?.cancel() }
    private fun bypass(host: String, patterns: String): Boolean = patterns.split(';').any {
        val pattern = it.trim()
        if (pattern.equals("<local>", true)) !host.contains('.') else
            pattern.isNotEmpty() && Regex(pattern.split('*').joinToString(".*") { part -> Regex.escape(part) }, RegexOption.IGNORE_CASE).matches(host)
    }
    private fun connection(url: URL, spec: JSONObject): HttpURLConnection {
        val type = spec.getInt("access")
        if (type == 1 || bypass(url.host, spec.optString("bypass"))) return url.openConnection(Proxy.NO_PROXY) as HttpURLConnection
        if (type == 3) {
            val pieces = spec.getString("proxy").split(';')
            val configured = pieces.firstOrNull { it.startsWith(url.protocol + "=", true) }?.substringAfter('=')
                ?: pieces.firstOrNull { !it.contains('=') } ?: throw IllegalArgumentException("Unsupported proxy configuration")
            val proxy = URI(if (configured.contains("://")) configured else "http://$configured")
            return url.openConnection(Proxy(Proxy.Type.HTTP, InetSocketAddress(proxy.host, if (proxy.port < 0) 80 else proxy.port))) as HttpURLConnection
        }
        return url.openConnection() as HttpURLConnection
    }
    private fun sameOrigin(a: URL, b: URL) = a.protocol.equals(b.protocol, true) && a.host.equals(b.host, true) &&
        (if (a.port < 0) a.defaultPort else a.port) == (if (b.port < 0) b.defaultPort else b.port)
    private fun execute(r: Request, spec: JSONObject, originalBody: ByteArray) {
        val server = spec.getString("server")
        val host = if (server.contains(':') && !server.startsWith('[')) "[$server]" else server
        var url = URL("${if (spec.getBoolean("secure")) "https" else "http"}://$host:${spec.getInt("port")}${spec.getString("path")}")
        val origin = url
        val flags = spec.getLong("flags")
        var method = spec.getString("method")
        var body = originalBody
        val headers = mutableListOf<Pair<String, String>>()
        val array = spec.getJSONArray("headers")
        for (i in 0 until array.length()) { val pair = array.getJSONArray(i); headers.add(pair.getString(0) to pair.getString(1)) }
        for (redirect in 0..10) {
            if (r.closed || r.client.closed) throw InterruptedException()
            val c = connection(url, r.client.spec); r.connection = c
            if (r.closed || r.client.closed) throw InterruptedException()
            c.connectTimeout = 30000; c.readTimeout = 30000; c.useCaches = false; c.instanceFollowRedirects = false
            c.requestMethod = method
            if (!headers.any { it.first.equals("User-Agent", true) }) c.setRequestProperty("User-Agent", r.client.spec.getString("agent"))
            if (!headers.any { it.first.equals("Accept-Encoding", true) }) c.setRequestProperty("Accept-Encoding", "identity")
            headers.forEach { (key, value) -> c.addRequestProperty(key, value) }
            if (flags and 0x80000000L != 0L) c.setRequestProperty("Cache-Control", "no-cache")
            if (flags and 0x00080000L == 0L) r.client.cookies.get(url.toURI(), emptyMap()).forEach { (key, values) -> values.forEach { c.addRequestProperty(key, it) } }
            if (sameOrigin(url, origin) && flags and 0x00040000L == 0L && spec.getString("user").isNotEmpty() && !headers.any { it.first.equals("Authorization", true) }) {
                val credentials = "${spec.getString("user")}:${spec.getString("password")}".toByteArray(Charsets.ISO_8859_1)
                c.setRequestProperty("Authorization", "Basic " + android.util.Base64.encodeToString(credentials, android.util.Base64.NO_WRAP))
            }
            if (body.isNotEmpty()) { c.doOutput = true; c.setFixedLengthStreamingMode(body.size); c.outputStream.use { it.write(body) } }
            if (r.closed) throw InterruptedException()
            val status = c.responseCode
            if (flags and 0x00080000L == 0L) {
                r.client.cookies.put(url.toURI(), c.headerFields)
                if (r.client.cookies.cookieStore.cookies.size > 512) r.client.cookies.cookieStore.removeAll()
            }
            val location = c.getHeaderField("Location")
            if (status in listOf(301, 302, 303, 307, 308) && location != null && flags and 0x00200000L == 0L) {
                if (redirect == 10) { r.error = 12156; return }
                val next = URL(url, location)
                if (next.protocol !in listOf("http", "https") || (url.protocol == "https" && next.protocol == "http" && flags and 0x00008000L == 0L)) { r.error = 12156; return }
                if (!sameOrigin(url, next)) headers.removeAll { it.first.equals("Authorization", true) || it.first.equals("Cookie", true) }
                if (status == 303 || ((status == 301 || status == 302) && method == "POST")) { method = "GET"; body = ByteArray(0); headers.removeAll { it.first.equals("Content-Type", true) || it.first.equals("Content-Length", true) } }
                c.disconnect(); r.connection = null; url = next; continue
            }
            val responseHeaders = JSONArray(); var total = 0
            c.headerFields.forEach { (key, values) -> if (key != null) values.asReversed().forEach {
                total += key.length + it.length + 4
                if (total > 65536) throw ProtocolException("Response headers too large")
                responseHeaders.put(JSONArray().put(key).put(it))
            } }
            val first = (c.getHeaderField(null) ?: "HTTP/1.1 $status").split(' ', limit = 3)
            r.head = JSONObject().put("status", status).put("version", first[0]).put("reason", if (first.size >= 3) first[2] else "").put("headers", responseHeaders).toString()
            val input = if (status >= 400) c.errorStream else c.inputStream
            input?.use { val bytes = ByteArray(8192); while (!r.closed) { val n = it.read(bytes); if (n < 0) break; if (n > 0) r.push(bytes, n) } }
            return
        }
    }
}
