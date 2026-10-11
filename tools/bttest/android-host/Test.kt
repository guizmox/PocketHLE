package com.pockethle.app
import org.json.JSONObject
import android.bluetooth.BluetoothAdapter

private fun call(op: String, id: Int = 0, extra: JSONObject = JSONObject()): JSONObject =
    JSONObject(RfcommHost.call(extra.put("op", op).put("id", id).toString()))
private fun ok(op: String, id: Int = 0, extra: JSONObject = JSONObject()): JSONObject {
    val r = call(op, id, extra); check(!r.has("error_code")) { "$op: $r" }; return r
}
private fun error(code: Int, op: String, id: Int, extra: JSONObject = JSONObject()) {
    check(call(op, id, extra).getInt("error_code") == code)
}
private fun addr(port: Int, remote: Long = 0) = JSONObject().put("addr", JSONObject()
    .put("address", remote).put("port", port).put("uuid", "00000000-0000-0000-0000-000000000000"))
private fun await(f: () -> Boolean) {
    val end = System.nanoTime() + 3_000_000_000L
    while (!f()) { check(System.nanoTime() < end) { "Timed out" }; Thread.sleep(2) }
}
fun main() {
    RfcommHost.initialize()
    repeat(100) {
        val server = ok("new").getInt("id")
        error(10022, "listen", server)
        ok("bind", server, addr(2)); ok("listen", server, JSONObject().put("backlog", 8))
        error(10035, "accept", server)
        val client = ok("new").getInt("id")
        error(10035, "connect", client, addr(2, 0x123456789abc))
        await { ok("ready", server).getBoolean("read") && ok("ready", client).getBoolean("write") }
        val accepted = ok("accept", server)
        val child = accepted.getInt("id")
        check(accepted.getJSONObject("addr").getLong("address") == 0x123456789abc)
        val data = ByteArray(4096) { (it % 251).toByte() }
        check(RfcommHost.write(client, data) == data.size)
        await { ok("available", child).getInt("count") == data.size }
        check(RfcommHost.read(child, data.size, true).contentEquals(data))
        check(ok("available", child).getInt("count") == data.size)
        check(RfcommHost.read(child, data.size, false).contentEquals(data))
        check(ok("available", child).getInt("count") == 0)
        check(!ok("ready", child).getBoolean("read"))
        check(RfcommHost.write(child, byteArrayOf(42)) == 1)
        await { ok("available", client).getInt("count") == 1 }
        check(RfcommHost.read(client, 1, false).contentEquals(byteArrayOf(42)))
        ok("shutdown", child, JSONObject().put("how", 1))
        check(RfcommHost.write(child, byteArrayOf(1)) == -10058)
        RfcommHost.closeAll()
        await { BluetoothAdapter.live.get() == 0 }
        error(10038, "ready", client)
        error(10091, "new", 0)
        RfcommHost.initialize()
    }
    // Hard handle cap, and cancellation while native connect is pending.
    repeat(64) { ok("new") }
    error(10024, "new", 0)
    RfcommHost.closeAll(); RfcommHost.initialize()
    BluetoothAdapter.connectDelayMs = 5000
    val connecting = ok("new").getInt("id")
    error(10035, "connect", connecting, addr(2, 0x123456789abc))
    RfcommHost.closeAll(); await { BluetoothAdapter.live.get() == 0 }
    BluetoothAdapter.connectDelayMs = 0
    RfcommHost.initialize()
    // Pending connection failure is surfaced through SO_ERROR/select.
    val bad = ok("new").getInt("id")
    error(10035, "connect", bad, addr(3, 0x123456789abc))
    await { ok("ready", bad).getBoolean("error") }
    check(ok("error", bad).getInt("code") == 10061)
    RfcommHost.closeAll(); await { BluetoothAdapter.live.get() == 0 }
    RfcommHost.initialize()
    // Listener teardown releases accepted-but-unclaimed peers as well.
    val server = ok("new").getInt("id")
    ok("bind", server, addr(2)); ok("listen", server)
    val client = ok("new").getInt("id")
    error(10035, "connect", client, addr(2, 0x123456789abc))
    await { ok("ready", server).getBoolean("read") }
    RfcommHost.closeAll(); await { BluetoothAdapter.live.get() == 0 }
    await { Thread.getAllStackTraces().keys.none { it.isAlive && it.name.startsWith("PocketHLE-RFCOMM-") } }
    check(BluetoothHost.discoverableRequests == 101)
    println("PASS: 100 RFCOMM exchanges, peek, shutdown, errors, pending accept, sockets and workers released")
}
