package com.pockethle.app

/** Physical-window corner controls and the clearance used by the control banks. */
internal object SnapshotControlGeometry {
    fun radius(density: Float) = (32f * density).toInt().coerceAtLeast(1)
    fun clearance(density: Float) = radius(density) + (8f * density).toInt().coerceAtLeast(1)
}
