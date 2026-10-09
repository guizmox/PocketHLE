package com.pockethle.app

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.ImageFormat
import android.hardware.camera2.*
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import androidx.core.content.ContextCompat
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger

/** Camera2 callbacks own all physical resources. JNI reads a bounded mailbox,
 * never a live Image or a camera buffer. A stopped opening attempt is closed
 * by its callback, including when the activity has already been destroyed. */
object CameraHost {
    private lateinit var context: Context
    private val thread = HandlerThread("pockethle-camera").apply { start() }
    private val handler = Handler(thread.looper)
    private val ids = AtomicInteger(1)
    private val sessions = ConcurrentHashMap<Int, Session>()
    @Volatile private var foreground = false
    fun initialize(ctx: Context) { context = ctx.applicationContext }
    fun permissions() = arrayOf(Manifest.permission.CAMERA)
    fun resume() { foreground = true; handler.post { sessions.values.forEach { it.start() } } }
    fun pause() { foreground = false; handler.post { sessions.values.forEach { it.release() } } }
    fun closeAll() { sessions.keys.toList().forEach { close(it) } }
    fun open(): Int {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED)
            throw SecurityException("Camera permission denied")
        val id = ids.getAndIncrement()
        val session = Session(); sessions[id] = session
        handler.post { session.start() }
        return id
    }
    fun read(id: Int): ByteArray {
        val s = sessions[id] ?: return errorPacket(6)
        if (!foreground) return ByteArray(0)
        if (s.error != 0) return errorPacket(s.error)
        return s.frame ?: ByteArray(0)
    }
    fun close(id: Int) {
        val s = sessions.remove(id) ?: return
        s.closed = true
        handler.post { s.release() }
    }
    private fun errorPacket(error: Int) = ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(error).array()
    private class Session {
        @Volatile var closed = false
        @Volatile var error = 0
        @Volatile var frame: ByteArray? = null
        private var opening = false
        private var generation = 0
        private var serial = 0L
        private var device: CameraDevice? = null
        private var capture: CameraCaptureSession? = null
        private var reader: ImageReader? = null
        fun release() {
            generation++; opening = false; frame = null
            try { capture?.stopRepeating() } catch (_: Exception) { }
            capture?.close(); capture = null
            device?.close(); device = null
            reader?.close(); reader = null
        }
        private fun valid(gen: Int) = !closed && foreground && generation == gen
        @Suppress("MissingPermission", "DEPRECATION")
        fun start() {
            if (closed || !foreground || opening || device != null) return
            error = 0; opening = true
            val gen = ++generation
            try {
                val manager = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
                val list = manager.cameraIdList
                val id = list.firstOrNull { manager.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == CameraCharacteristics.LENS_FACING_BACK }
                    ?: list.firstOrNull() ?: run { error = 2; opening = false; return }
                val map = manager.getCameraCharacteristics(id).get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
                val sizes = map?.getOutputSizes(ImageFormat.YUV_420_888)?.filter { it.width % 2 == 0 && it.height % 2 == 0 && it.width <= 1920 && it.height <= 1920 }
                val size = sizes?.firstOrNull { it.width == 640 && it.height == 480 }
                    ?: sizes?.minByOrNull { kotlin.math.abs(it.width * it.height - 640 * 480) }
                    ?: run { error = 50; opening = false; return }
                val images = ImageReader.newInstance(size.width, size.height, ImageFormat.YUV_420_888, 2)
                reader = images
                images.setOnImageAvailableListener({ source ->
                    try {
                        val image = source.acquireLatestImage() ?: return@setOnImageAvailableListener
                        try {
                            if (valid(gen)) {
                                val crop = image.cropRect
                                val w = crop.width(); val h = crop.height()
                                if (w % 2 != 0 || h % 2 != 0) { error = 13; return@setOnImageAvailableListener }
                                val packet = ByteBuffer.allocate(16 + w * h * 3 / 2).order(ByteOrder.LITTLE_ENDIAN)
                                packet.putInt(w).putInt(h).putLong(++serial)
                                for (p in 0..2) {
                                    val plane = image.planes[p]; val divisor = if (p == 0) 1 else 2
                                    val buffer = plane.buffer; val base = buffer.position()
                                    for (y in 0 until h / divisor) for (x in 0 until w / divisor) {
                                        packet.put(buffer.get(base + (crop.top / divisor + y) * plane.rowStride + (crop.left / divisor + x) * plane.pixelStride))
                                    }
                                }
                                frame = packet.array()
                            }
                        } finally { image.close() }
                    } catch (_: Exception) { if (valid(gen)) error = 13 }
                }, handler)
                manager.openCamera(id, object : CameraDevice.StateCallback() {
                    override fun onOpened(camera: CameraDevice) {
                        if (!valid(gen)) { camera.close(); return }
                        device = camera; opening = false
                        try {
                            camera.createCaptureSession(listOf(images.surface), object : CameraCaptureSession.StateCallback() {
                                override fun onConfigured(session: CameraCaptureSession) {
                                    if (!valid(gen)) { session.close(); return }
                                    capture = session
                                    try {
                                        val request = camera.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW).apply {
                                            addTarget(images.surface)
                                            set(CaptureRequest.CONTROL_MODE, CameraMetadata.CONTROL_MODE_AUTO)
                                        }
                                        session.setRepeatingRequest(request.build(), null, handler)
                                    } catch (_: Exception) { error = 21; release() }
                                }
                                override fun onConfigureFailed(session: CameraCaptureSession) {
                                    session.close(); if (valid(gen)) { error = 21; release() }
                                }
                            }, handler)
                        } catch (_: Exception) { error = 21; release() }
                    }
                    override fun onDisconnected(camera: CameraDevice) {
                        camera.close(); if (valid(gen)) { error = 1167; release() }
                    }
                    override fun onError(camera: CameraDevice, code: Int) {
                        camera.close(); if (valid(gen)) {
                            error = when (code) { ERROR_CAMERA_IN_USE, ERROR_MAX_CAMERAS_IN_USE -> 32; ERROR_CAMERA_DISABLED -> 5; else -> 21 }
                            release()
                        }
                    }
                }, handler)
            } catch (_: SecurityException) { error = 5; release() }
              catch (_: Exception) { error = 21; release() }
        }
    }
}
