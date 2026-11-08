package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class GraphicsTimingTest {
    @Test fun aggregatesOnlyAfterASecondAndResetsTheWindow() {
        val lines = mutableListOf<String>()
        val timing = GraphicsTiming(true) { lines += it }
        timing.record(GraphicsTiming.Stage.UPLOAD, 2_000_000, 1)
        timing.record(GraphicsTiming.Stage.UPLOAD, 4_000_000, 1_000_000_000)
        assertTrue(lines.isEmpty())
        timing.record(GraphicsTiming.Stage.PRESENT_GPU, 3_000_000, 1_000_000_001)
        assertEquals(2, lines.size)
        assertTrue(lines[0].contains("calls=2 total_ms=6.0 mean_ms=3.0 max_ms=4.0"))
        assertTrue(lines[1].contains("stage=present_gpu calls=1"))
        lines.clear()
        timing.record(GraphicsTiming.Stage.UPLOAD, 1_000_000, 2_000_000_001)
        assertEquals(1, lines.size)
        assertTrue(lines.single().contains("calls=1 total_ms=1.0"))
    }
    @Test fun disabledTimingProducesNoOutput() {
        val timing = GraphicsTiming(false) { error("disabled diagnostic emitted output") }
        assertEquals(0L, timing.start())
        timing.record(GraphicsTiming.Stage.DRAW, 100, 1)
        timing.record(GraphicsTiming.Stage.DRAW, 100, 2_000_000_001)
        timing.end(GraphicsTiming.Stage.DRAW, 0)
    }
    @Test fun diagnosticSettingDefaultsOffAndSurvivesConfigRoundTrip() {
        val oldConfig = LauncherConfig.fromJson(org.json.JSONObject())
        assertFalse(oldConfig.graphicsDiagnostics)
        val enabled = oldConfig.copy(graphicsDiagnostics = true)
        assertTrue(LauncherConfig.fromJson(enabled.toJson()).graphicsDiagnostics)
        assertFalse(LauncherConfig.fromJson(enabled.copy(graphicsDiagnostics = false).toJson()).graphicsDiagnostics)
        assertEquals(oldConfig.verbosity, enabled.verbosity)
        assertEquals(oldConfig.gpuRenderer, enabled.gpuRenderer)
    }
}
