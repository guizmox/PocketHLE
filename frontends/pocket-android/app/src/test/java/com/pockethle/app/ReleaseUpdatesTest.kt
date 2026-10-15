package com.pockethle.app

import org.json.JSONArray
import org.junit.Assert.assertEquals
import org.junit.Test

class ReleaseUpdatesTest {
    @Test fun sharedReleaseCases() {
        val data = javaClass.classLoader!!.getResourceAsStream("releases.json")!!.bufferedReader().use { it.readText() }
        val cases = JSONArray(data)
        for (index in 0 until cases.length()) {
            val case = cases.getJSONObject(index)
            val result = ReleaseUpdates.parse(case.getJSONObject("release").toString(), case.getString("installed"))
            assertEquals(case.getString("name"), case.getBoolean("available"), result != null)
            if (result != null) assertEquals(case.getString("name"), case.getString("url"), result.url)
        }
        assertEquals(null, ReleaseUpdates.parse("invalid JSON", "0.3.1"))
    }
}
