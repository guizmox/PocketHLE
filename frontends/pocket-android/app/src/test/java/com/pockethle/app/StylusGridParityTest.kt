package com.pockethle.app

import org.json.JSONArray
import org.junit.Assert.assertEquals
import org.junit.Test

class StylusGridParityTest {
    @Test fun sharedGridCases() {
        val resource = checkNotNull(javaClass.classLoader?.getResourceAsStream("stylus-grid.json"))
        val cases = JSONArray(resource.bufferedReader().use { it.readText() })
        for (index in 0 until cases.length()) {
            val case = cases.getJSONObject(index)
            val grid = StylusGridConfig.parse(case.getJSONObject("grid").toString())
            val input = case.getJSONObject("input")
            val fullscreen = if (case.isNull("fullscreen")) null else case.getString("fullscreen")
            val expected = if (case.isNull("cell")) null else case.getInt("cell")
            val cell = grid.cellFor(GridInput(input.getString("source"), input.getString("name")), fullscreen)
            assertEquals(case.getString("name"), expected, cell)
            if (cell != null) {
                val point = case.getJSONArray("point")
                assertEquals(point.getInt(0) to point.getInt(1), grid.point(cell))
            }
        }
    }
}
