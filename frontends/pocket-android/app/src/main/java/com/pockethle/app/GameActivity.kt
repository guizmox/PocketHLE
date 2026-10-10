package com.pockethle.app

import android.annotation.SuppressLint
import android.content.res.Configuration
import android.opengl.GLSurfaceView
import android.media.AudioManager
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.HapticFeedbackConstants
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.widget.ProgressBar
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.json.JSONObject
import android.content.pm.ActivityInfo
import android.view.KeyEvent
import android.content.pm.PackageManager
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat

/**
 * Hosts the emulator output for one game.
 *
 * The implementation drives the emulator session-style (see
 * `pocket-android-jni::runner`): a Rust worker thread runs the
 * emulator and the activity polls the latest framebuffer on the UI
 * thread roughly every 33 ms (~30 Hz), feeds touches and virtual
 * gamepad presses straight back into the kernel, and asks the
 * worker to stop on Back / `onDestroy`. The previous single-shot
 * `NativeBridge.runGame` API blocked until the emulator exited and
 * never streamed intermediate frames, which looked like an infinite
 * loading spinner once the real Unicorn backend was wired up.
 */
class GameActivity : AppCompatActivity() {
    private var hardwareStart: (() -> Unit)? = null
    private val hardwarePermissionRequest = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        val start = hardwareStart; hardwareStart = null; start?.invoke()
    }

    private lateinit var surface: GLSurfaceView
    private lateinit var progress: ProgressBar
    private lateinit var status: TextView
    private lateinit var gameControls: View
    private lateinit var glRenderer: AndroidFrameRenderer
    private var displayScale = 0
    private lateinit var bindings: InputBindings

    /** Cached handle from `nativeStartGame` (`0` once we've finished). */
    @Volatile private var session: Long = 0
    private var joiningSession = false

    /** Most recent framebuffer the worker produced — held so we can
     * repaint after `surfaceChanged` resizes the SurfaceView even if
     * the worker has not produced a new frame yet. */
    private var lastFrame: FrameSnapshot? = null

    @Volatile private var audioRunning = false
    @Volatile private var audioGeneration = 0L
    private val audioThreads = mutableListOf<Thread>()
    private var audioThread: Thread? = null
    private var audioTrack: AudioTrack? = null

    private var controlsOpacity: Float = 1f

    /**
     * Presentation-only quarter turn from `GameSettings::rotation`,
     * in degrees clockwise.
     *
     * JumpyBall (and Asphalt 2's Motorola Q build) is laid out for a
     * landscape screen but only renders correctly while it believes it
     * is on a 240x320 portrait panel — forcing `screen` to 320x240
     * breaks it. Keeping the guest portrait and rotating the *picture*
     * gives the intended landscape view, which is what the desktop
     * launcher's "Rotate display" combo already does.
     */
    private var rotationDegrees: Int = 0

    /** Android keycode -> guest virtual key currently held. */
    private val heldPhysicalKeys = HashMap<Int, Int>()
    /** Guest virtual keys held by either physical or on-screen controls. */
    private val heldGuestKeys = HashMap<Int, Int>()
    private val heldVirtualKeys = HashSet<Int>()
    /** Guest keys currently asserted by a gamepad stick or hat. */
    private val heldAxisKeys = HashMap<String, Int>()
    private var surfacePointerDown = false
    private var lastPointerX = 0
    private var lastPointerY = 0

    private val mainHandler = Handler(Looper.getMainLooper())

    /** Polling tick. ~30 Hz keeps the SurfaceView smooth without
     * burning the CPU on a phone. */
    private val pollTick = object : Runnable {
        override fun run() {
            if (session == 0L) return
            val raw = NativeBridge.nativePollFrame(session)
            if (raw != null) {
                decodeFrame(raw)?.let { frame ->
                    lastFrame = frame
                    paintFrame(frame)
                }
            }
            if (NativeBridge.nativeIsRunning(session) == 0) {
                // Worker exited on its own (game called ExitProcess
                // / hit max_slices / errored out). Reap it so we
                // surface the summary in the status panel.
                finishSession()
                return
            }
            mainHandler.postDelayed(this, POLL_INTERVAL_MS)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val config = readLauncherConfig()
        bindings = InputBindings(config)
        displayScale = config.displayScale
        controlsOpacity = config.controlsOpacity
        requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        setContentView(R.layout.activity_game)
        gameControls = findViewById(R.id.game_controls)
        gameControls.alpha = controlsOpacity
        androidx.core.view.WindowCompat.setDecorFitsSystemWindows(window, false)
        hideSystemBars()

        val name = intent.getStringExtra(EXTRA_GAME_NAME) ?: "PocketHLE"
        title = name

        surface = findViewById(R.id.surface)
        glRenderer = AndroidFrameRenderer(this, config.upscaleFilter, displayScale)
        surface.setEGLContextClientVersion(3)
        surface.setRenderer(glRenderer)
        surface.renderMode = GLSurfaceView.RENDERMODE_WHEN_DIRTY
        progress = findViewById(R.id.progress)
        status = findViewById(R.id.status)

        findViewById<View>(R.id.btn_stop_emulation).apply {
            setOnTouchListener { view, event ->
                if (event.actionMasked == MotionEvent.ACTION_DOWN) {
                    view.performHapticFeedback(HapticFeedbackConstants.VIRTUAL_KEY)
                }
                false // Preserve normal pressed-state, cancellation and click handling.
            }
            setOnClickListener {
                it.isEnabled = false
                finishSession()
            }
        }
        androidx.core.view.ViewCompat.setOnApplyWindowInsetsListener(findViewById(R.id.game_root)) { view, insets ->
            val safe = insets.getInsets(androidx.core.view.WindowInsetsCompat.Type.displayCutout())
            view.setPadding(safe.left, safe.top, safe.right, safe.bottom); insets
        }
        status.visibility = View.GONE

        wireSurfaceTouchInput()
        wireVirtualGamepad()

        val id = intent.getStringExtra(EXTRA_GAME_ID)
        if (id == null) {
            android.widget.Toast.makeText(this, getString(R.string.run_failed_no_id), android.widget.Toast.LENGTH_LONG).show()
            finish()
            return
        }

        val rootDir = LibraryPaths.root(this)
        val isGizmondo = NativeBridge.isGizmondoGame(rootDir, id)
        val fixedGps = config.gpsFixedEnabled && isGizmondo
        rotationDegrees = if (isGizmondo) 0 else readRotationDegrees(rootDir, id)
        glRenderer.setRotationDegrees(rotationDegrees)
        BluetoothHost.initialize(this)
        CameraHost.initialize(this)
        GpsHost.initialize(this)
        val permissions = (if (config.bluetoothEnabled) BluetoothHost.permissions().toList() else emptyList()) +
            (if (config.cameraEnabled) CameraHost.permissions().toList() else emptyList()) +
            (if (config.gpsEnabled && !fixedGps) GpsHost.permissions().toList() else emptyList())
        val missing = permissions.filter { ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED }
        if (missing.isNotEmpty()) {
            hardwareStart = { startSession(rootDir, id) }
            // Android12 requires coarse and fine in the same precise-location request.
            val requested = if (config.gpsEnabled && !fixedGps && missing.contains(android.Manifest.permission.ACCESS_FINE_LOCATION))
                (missing + GpsHost.permissions()).distinct() else missing
            hardwarePermissionRequest.launch(requested.toTypedArray())
        } else startSession(rootDir, id)
    }

    private fun startSession(rootDir: String, id: String) {
        if (isFinishing || isDestroyed) return
        val handle = NativeBridge.nativeStartGame(rootDir, id)
        if (handle == 0L) {
            progress.visibility = View.GONE
            android.widget.Toast.makeText(this, "Could not start the game: see logcat", android.widget.Toast.LENGTH_LONG).show()
            finish()
            return
        }
        session = handle
        startAudio(handle)
        status.text = "Backend: Unicorn (ARM)\nRunning…"
        // The spinner gets hidden the moment the first frame arrives.
        mainHandler.postDelayed(pollTick, POLL_INTERVAL_MS)
    }

    /**
     * The activity declares `configChanges="orientation|screenSize"`, so
     * rotating the device does *not* re-inflate the layout — the running
     * emulator session and its GL surface survive. That means the
     * portrait/landscape difference has to be applied by hand here:
     * pad the game area above the controls in portrait, let the picture
     * run full-height under them in landscape.
     */
    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        hideSystemBars()
        // Re-submit the last frame so the picture is re-letterboxed for
        // the new window shape even if the guest is between frames. This
        // deliberately does not go through `paintFrame`, which would
        // count a rotation as a rendered frame in the FPS overlay.
        lastFrame?.let {
            glRenderer.submit(it)
            surface.requestRender()
        }
    }

    override fun onResume() {
        super.onResume()
        if (::surface.isInitialized) surface.onResume()
        CameraHost.resume()
        GpsHost.resume()
        val handle = session
        if (handle != 0L && !audioRunning) startAudio(handle)
    }

    override fun onPause() {
        if (::surface.isInitialized) surface.onPause()
        CameraHost.pause()
        GpsHost.pause()
        releaseHeldInput()
        stopAudio()
        super.onPause()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) hideSystemBars()
    }

    override fun onSupportNavigateUp(): Boolean {
        finish()
        return true
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (joiningSession) return

        // Ask the emulator to wind down gracefully; the polling
        // tick will notice the worker exited and call finishSession.
        if (session != 0L) {
            NativeBridge.nativeRequestStop(session)
        }
        if (session != 0L) finishSession() else finish()
    }

    override fun onDestroy() {
        CameraHost.closeAll()
        GpsHost.closeAll()
        finishSession()
        mainHandler.removeCallbacksAndMessages(null)
        surface.onPause()
        super.onDestroy()
    }

    /**
     * Stop the emulator if it is still running, free the native
     * session, and surface the textual summary in the status panel.
     */
    private fun finishSession() {
        val handle = session
        if (handle == 0L) return
        joiningSession = true
        CameraHost.closeAll()
        GpsHost.closeAll()
        releaseHeldInput()
        session = 0
        stopAudio()
        val audioToJoin = audioThreads.toList()
        progress.visibility = View.GONE
        // `nativeFinishGame` blocks on the worker thread join, so
        // do it off the UI thread to keep the UI responsive — the
        // join is usually fast (the Stop signal already fired) but
        // a long emulator slice can drag it out a few hundred ms.
        Thread {
            NativeBridge.nativeRequestStop(handle)
            audioToJoin.forEach { it.join() }
            val summary = NativeBridge.nativeFinishGame(handle)
            mainHandler.post {
                joiningSession = false
                android.util.Log.i("PocketHLE", summary)
                if (!isFinishing && !isDestroyed) {
                    android.widget.Toast.makeText(this, "Emulation finished. See the logs for details.", android.widget.Toast.LENGTH_SHORT).show()
                    finish()
                }
            }
        }.start()
    }

    private fun startAudio(handle: Long) {
        stopAudio()
        audioRunning = true
        val generation=audioGeneration
        audioThreads.removeAll { !it.isAlive }
        audioThread = Thread({
            var track: AudioTrack? = null
            try {
                android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_AUDIO)
                while (audioRunning && generation == audioGeneration && session == handle) {
                    var packed = 0L
                    while (audioRunning && generation == audioGeneration && session == handle && packed == 0L) {
                        packed = NativeBridge.nativeAudioFormat(handle)
                        if (packed == 0L) Thread.sleep(20)
                    }
                    if (!audioRunning || generation != audioGeneration || session != handle || packed == 0L) {
                        android.util.Log.w("PocketHLE", "Audio format was not announced by the guest")
                        return@Thread
                    }
                    val rate = (packed ushr 16).toInt().coerceIn(8000, 48000)
                    val channels = (packed and 0xffff).toInt().coerceIn(1, 2)
                    val manager = getSystemService(android.content.Context.AUDIO_SERVICE) as AudioManager
                    val outputRate = manager.getProperty(AudioManager.PROPERTY_OUTPUT_SAMPLE_RATE)
                        ?.toIntOrNull()?.takeIf { it in 8000..192000 }
                        ?: AudioTrack.getNativeOutputSampleRate(AudioManager.STREAM_MUSIC)
                            .takeIf { it in 8000..192000 } ?: rate
                    val resampler = AudioResampler(rate, outputRate, channels)
                    val channelMask = if (channels == 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
                    val minBuffer = AudioTrack.getMinBufferSize(outputRate, channelMask, AudioFormat.ENCODING_PCM_16BIT)
                    // Small pulls let live circular mixers observe intermediate playback positions.
                    // Colors uses a 4096-sample loop: a 4096-sample pull skips a full turn.
                    val pullSamples = maxOf(1, rate / 100) * channels
                    val targetFrames = maxOf(1, outputRate / 100) * 2 // Approximately 20 ms.
                    // Allocate enough for Android's minimum, then limit the effective queue.
                    val bufferSize = maxOf(minBuffer.takeIf { it > 0 } ?: 0, targetFrames * channels * 2)
                    val trackBuilder = AudioTrack.Builder()
                        .setAudioAttributes(AudioAttributes.Builder()
                            .setUsage(AudioAttributes.USAGE_GAME)
                            .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                            .build())
                        .setAudioFormat(AudioFormat.Builder()
                            .setSampleRate(outputRate)
                            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                            .setChannelMask(channelMask)
                            .build())
                        .setBufferSizeInBytes(bufferSize)
                        .setTransferMode(AudioTrack.MODE_STREAM)
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                        trackBuilder.setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                    }
                    track = trackBuilder.build()
                    if (track?.state != AudioTrack.STATE_INITIALIZED) {
                        android.util.Log.e("PocketHLE", "AudioTrack was not initialized")
                        return@Thread
                    }
                    val selectedFrames = track.setBufferSizeInFrames(targetFrames)
                    val actualFrames = track.bufferSizeInFrames
                    val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) track.performanceMode else 0
                    audioTrack = track
                    track.play()
                    android.util.Log.i("PocketHLE", "AudioTrack started: ${outputRate}Hz (guest=${rate}Hz), ${channels}ch, " +
                        "bufferFrames=$actualFrames (~${actualFrames * 1000 / outputRate}ms), " +
                        "requestedFrames=$targetFrames result=$selectedFrames performance=$mode")
                    while (audioRunning && generation == audioGeneration && session == handle) {
                        val currentFormat = NativeBridge.nativeAudioFormat(handle)
                        if (currentFormat != packed) {
                            android.util.Log.i("PocketHLE", "Audio guest format changed: $packed -> $currentFormat; rebuilding output")
                            break
                        }
                        val pcm = NativeBridge.nativePollAudio(handle, pullSamples)
                        // The foreground child can change while the JNI pull is in flight.
                        // Never interpret its first PCM chunk using the parent's format.
                        if (NativeBridge.nativeAudioFormat(handle) != packed) break
                        if (pcm != null && pcm.isNotEmpty()) {
                            writeAudio(track, resampler.convert(pcm), generation)
                        } else {
                            writeSilence(track, maxOf(channels * outputRate / 200, 256), generation)
                            Thread.sleep(5)
                        }
                    }
                    // Drop queued parent PCM and reset interpolation for the new format.
                    track.pause()
                    track.flush()
                    track.release()
                    if (audioTrack === track) audioTrack = null
                    track = null
                }
            } catch (error: Throwable) {
                android.util.Log.e("PocketHLE", "AudioTrack playback failed", error)
            } finally {
                try { track?.pause() } catch (_: Throwable) {}
                try { track?.flush() } catch (_: Throwable) {}
                try { track?.release() } catch (_: Throwable) {}
                if (audioTrack === track) audioTrack = null
            }
        }, "pockethle-audio")
        audioThread?.let { audioThreads.add(it);it.start() }
    }

    private fun stopAudio() {
        audioRunning = false
        audioGeneration++
        runCatching { audioTrack?.pause() }
        runCatching { audioTrack?.flush() }
        val oldThread = audioThread
        audioThread = null
        audioTrack = null
        oldThread?.interrupt()
        if (oldThread !== Thread.currentThread()) {
            try { oldThread?.join(250) } catch (_: InterruptedException) { Thread.currentThread().interrupt() }
        }
    }

    private fun writeAudio(track: AudioTrack, pcm: ShortArray, generation: Long) {
        var offset = 0
        while (offset < pcm.size && audioRunning && generation == audioGeneration) {
            val written = track.write(pcm, offset, pcm.size - offset, AudioTrack.WRITE_BLOCKING)
            if (written <= 0) return
            offset += written
        }
    }

    private fun writeSilence(track: AudioTrack, samples: Int, generation: Long) {
        writeAudio(track, ShortArray(samples), generation)
    }

    // -------------------------------------------------------------------
    // Surface rendering
    // -------------------------------------------------------------------

    private fun decodeFrame(raw: ByteArray): FrameSnapshot? {
        if (raw.size < 8) return null
        val buf = ByteBuffer.wrap(raw).order(ByteOrder.LITTLE_ENDIAN)
        val w = buf.int
        val h = buf.int
        if (w <= 0 || h <= 0) return null
        val pixelBytes = w * h * 4
        if (raw.size < 8 + pixelBytes) return null
        return FrameSnapshot(w, h, raw, 8)
    }

    private fun paintFrame(frame: FrameSnapshot) {
        progress.visibility = View.GONE
        glRenderer.submit(frame)
        surface.requestRender()

    }

    /**
     * Read `GameSettings::rotation` for this game and turn it into
     * degrees clockwise. Presentation only: the guest keeps rendering at
     * whatever `screen` says.
     */
    private fun readRotationDegrees(rootDir: String, id: String): Int {
        val raw = NativeBridge.readGameSettings(rootDir, id)
        val settings = runCatching {
            val obj = JSONObject(raw)
            if (obj.has("ok") && !obj.optBoolean("ok", true)) GameSettings.default()
            else GameSettings.fromJson(obj)
        }.getOrDefault(GameSettings.default())
        return when (settings.rotation) {
            "cw90" -> 90
            "half" -> 180
            "ccw90" -> 270
            else -> 0
        }
    }

    private fun hideSystemBars() {
        androidx.core.view.WindowInsetsControllerCompat(window, window.decorView).apply {
            systemBarsBehavior = androidx.core.view.WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(androidx.core.view.WindowInsetsCompat.Type.systemBars())
        }
    }

    private fun readLauncherConfig(): LauncherConfig {
        val raw = NativeBridge.readConfig(LibraryPaths.root(this))
        return runCatching {
            val obj = JSONObject(raw)
            if (obj.has("ok") && !obj.optBoolean("ok", true)) LauncherConfig.default()
            else LauncherConfig.fromJson(obj)
        }.getOrDefault(LauncherConfig.default())
    }

    // -------------------------------------------------------------------
    // Input plumbing
    // -------------------------------------------------------------------

    /**
     * Android controller keys arrive through the activity key dispatch path,
     * not through the SurfaceView touch listener. Dispatch them before child
     * views consume them and use the same guest codes as the virtual pad.
     * Counts are kept per guest key because Android can report, for example,
     * Y and Start as two different physical keys that share the Gizmondo
     * Start code.
     */
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (event.keyCode == KeyEvent.KEYCODE_F10) {
            if (event.action == KeyEvent.ACTION_DOWN && event.repeatCount == 0) { glRenderer.requestScreenshot(); surface.requestRender() }
            return true
        }
        val sources = event.device?.sources ?: event.source
        val isController = sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD || sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
        val vk = (if (isController) InputBindings.controllerName(event.keyCode)?.let { bindings.controllerKey(it) }
            else if (InputBindings.keyName(event) != null) bindings.keyboardKey(event) else virtualKeyFor(event.keyCode)) ?: return super.dispatchKeyEvent(event)
        val handle = session
        if (handle == 0L) return super.dispatchKeyEvent(event)
        when (event.action) {
            KeyEvent.ACTION_DOWN -> {
                if (event.repeatCount == 0 && heldPhysicalKeys.putIfAbsent(event.keyCode, vk) == null) {
                    acquireGuestKey(handle, vk)
                }
            }
            KeyEvent.ACTION_UP -> {
                heldPhysicalKeys.remove(event.keyCode)?.let { releaseGuestKey(handle, it) }
            }
        }
        return true
    }

    private fun virtualKeyFor(keyCode: Int): Int? = when (keyCode) {
        KeyEvent.KEYCODE_DPAD_UP -> VK_UP
        KeyEvent.KEYCODE_DPAD_DOWN -> VK_DOWN
        KeyEvent.KEYCODE_DPAD_LEFT -> VK_LEFT
        KeyEvent.KEYCODE_DPAD_RIGHT -> VK_RIGHT
        KeyEvent.KEYCODE_ENTER -> VK_RETURN
        KeyEvent.KEYCODE_SPACE -> VK_SPACE
        KeyEvent.KEYCODE_TAB -> VK_TAB
        KeyEvent.KEYCODE_ESCAPE -> VK_ESCAPE
        KeyEvent.KEYCODE_SHIFT_LEFT, KeyEvent.KEYCODE_SHIFT_RIGHT -> VK_SHIFT
        KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.KEYCODE_CTRL_RIGHT -> VK_CTRL
        KeyEvent.KEYCODE_BUTTON_A -> VK_RETURN
        KeyEvent.KEYCODE_BUTTON_B -> VK_SPACE
        KeyEvent.KEYCODE_BUTTON_X -> VK_SHIFT
        KeyEvent.KEYCODE_BUTTON_Y -> VK_CTRL
        KeyEvent.KEYCODE_BUTTON_START -> VK_ESCAPE
        KeyEvent.KEYCODE_BUTTON_SELECT -> VK_TAB
        KeyEvent.KEYCODE_BUTTON_L1, KeyEvent.KEYCODE_BUTTON_L2 -> VK_TAB
        KeyEvent.KEYCODE_BUTTON_R1, KeyEvent.KEYCODE_BUTTON_R2 -> VK_ESCAPE
        KeyEvent.KEYCODE_F1 -> VK_F1
        KeyEvent.KEYCODE_F2 -> VK_F2
        KeyEvent.KEYCODE_F3 -> VK_F3
        KeyEvent.KEYCODE_F4 -> 0x73
        KeyEvent.KEYCODE_F11 -> VK_F11
        else -> null
    }

    /**
     * A DualShock 4 (and most other pads) reports its left stick and its
     * hat switch as motion axes, not as `KEYCODE_DPAD_*` keys, so the
     * D-pad would do nothing without this. Both are folded into the same
     * guest arrow keys the on-screen pad sends, through the same
     * refcounted acquire/release pair, so a stick and the on-screen
     * button can be held at once without one release cancelling both.
     */
    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        val joystick = event.source and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK ||
            event.source and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD
        val handle = session
        if (!joystick || handle == 0L || event.action != MotionEvent.ACTION_MOVE) {
            return super.onGenericMotionEvent(event)
        }
        val desired = linkedMapOf<String,Int>()
        fun axis(name: String,value: Float,negative: String,positive: String) {
            if(value <= -AXIS_DEADZONE) bindings.controllerKey(name+negative)?.let { desired[name+negative]=it }
            if(value >= AXIS_DEADZONE) bindings.controllerKey(name+positive)?.let { desired[name+positive]=it }
        }
        axis("DPad",event.getAxisValue(MotionEvent.AXIS_HAT_X),"Left","Right")
        axis("DPad",event.getAxisValue(MotionEvent.AXIS_HAT_Y),"Up","Down")
        axis("LeftStick",event.getAxisValue(MotionEvent.AXIS_X),"Left","Right")
        axis("LeftStick",event.getAxisValue(MotionEvent.AXIS_Y),"Up","Down")
        axis("RightStick",event.getAxisValue(MotionEvent.AXIS_Z),"Left","Right")
        axis("RightStick",event.getAxisValue(MotionEvent.AXIS_RZ),"Up","Down")
        listOf("LeftTrigger2" to MotionEvent.AXIS_LTRIGGER,"RightTrigger2" to MotionEvent.AXIS_RTRIGGER).forEach { (name,axis) ->
            if(event.getAxisValue(axis)>.60f) bindings.controllerKey(name)?.let { desired[name]=it }
        }
        heldAxisKeys.keys.toList().filter { it !in desired }.forEach { name -> heldAxisKeys.remove(name)?.let { releaseGuestKey(handle,it) } }
        desired.forEach { (name,vk) -> if (heldAxisKeys.putIfAbsent(name,vk)==null) acquireGuestKey(handle,vk) }
        return true
    }

    private fun acquireGuestKey(handle: Long, vk: Int) {
        val count = heldGuestKeys[vk] ?: 0
        heldGuestKeys[vk] = count + 1
        if (count == 0) {
            NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_KEY_DOWN, vk, 0)
        }
    }

    private fun releaseGuestKey(handle: Long, vk: Int) {
        val count = heldGuestKeys[vk] ?: return
        if (count <= 1) {
            heldGuestKeys.remove(vk)
            NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_KEY_UP, vk, 0)
        } else {
            heldGuestKeys[vk] = count - 1
        }
    }

    /** Release keys and pointer state before the native session can outlive the UI. */
    private fun releaseHeldInput() {
        val handle = session
        if (handle != 0L) {
            for (vk in heldGuestKeys.keys.toList()) {
                NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_KEY_UP, vk, 0)
            }
            if (surfacePointerDown) {
                NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_POINTER_UP, lastPointerX, lastPointerY)
            }
        }
        heldPhysicalKeys.clear()
        heldGuestKeys.clear()
        (gameControls as? GizmondoControls)?.clearPressedControls()
        heldVirtualKeys.clear()
        heldAxisKeys.clear()
        surfacePointerDown = false
    }

    /**
     * Forward any touches on the framebuffer surface as
     * `WM_LBUTTONDOWN` / `WM_LBUTTONUP` events with stylus
     * coordinates in 240×320 game space — the same mapping the
     * desktop GUI uses.
     */
    @SuppressLint("ClickableViewAccessibility")
    private fun wireSurfaceTouchInput() {
        surface.setOnTouchListener { v, event ->
            val handle = session
            if (handle == 0L) return@setOnTouchListener false
            val frame = lastFrame
            val mapped = frame?.let { mapTouchToGame(v, event, it) }
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    val (gx, gy) = mapped ?: return@setOnTouchListener true
                    NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_POINTER_DOWN, gx, gy)
                    surfacePointerDown = true
                    lastPointerX = gx
                    lastPointerY = gy
                }
                MotionEvent.ACTION_MOVE -> {
                    if (surfacePointerDown && mapped != null) {
                        val (gx, gy) = mapped
                        NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_POINTER_MOVE, gx, gy)
                        lastPointerX = gx
                        lastPointerY = gy
                    }
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    if (surfacePointerDown) {
                        val (gx, gy) = mapped ?: (lastPointerX to lastPointerY)
                        NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_POINTER_UP, gx, gy)
                        surfacePointerDown = false
                        lastPointerX = gx
                        lastPointerY = gy
                        if (event.actionMasked == MotionEvent.ACTION_UP) v.performClick()
                    }
                }
            }
            true
        }
    }

    /**
     * j2me-loader-inspired virtual gamepad: a D-pad on the left and
     * three action / soft-key buttons on the right. The button views
     * live in `activity_game.xml`. We listen for touch events
     * directly so the WM_KEYDOWN/WM_KEYUP pair is fired as the user
     * presses and releases the button — not just once per click.
     */
    private fun wireVirtualGamepad() {
        bindVk(R.id.btn_up, VK_UP)
        bindVk(R.id.btn_down, VK_DOWN)
        bindVk(R.id.btn_left, VK_LEFT)
        bindVk(R.id.btn_right, VK_RIGHT)
        bindVk(R.id.btn_action, VK_RETURN)
        bindVk(R.id.btn_piano1, 0x70)
        bindVk(R.id.btn_piano2, 0x71)
        bindVk(R.id.btn_piano3, 0x72)
        bindVk(R.id.btn_piano4, 0x73)
        bindVk(R.id.btn_piano5, 0x7A)
        bindVk(R.id.btn_a, VK_CTRL)
        bindVk(R.id.btn_b, VK_SPACE)
        bindVk(R.id.btn_c, VK_SHIFT)
        bindVk(R.id.btn_soft1, VK_TAB)
        bindVk(R.id.btn_soft2, VK_ESCAPE)
    }

    @SuppressLint("ClickableViewAccessibility")
    private fun bindVk(viewId: Int, vk: Int) {
        val btn = findViewById<View?>(viewId) ?: return
        btn.setOnTouchListener { v, event ->
            val handle = session
            if (handle == 0L) return@setOnTouchListener false
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    if (heldVirtualKeys.add(vk)) {
                        acquireGuestKey(handle, vk)
                        v.performHapticFeedback(HapticFeedbackConstants.VIRTUAL_KEY)
                    }
                    v.isPressed = true
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    if (heldVirtualKeys.remove(vk)) releaseGuestKey(handle, vk)
                    v.isPressed = false
                    if (event.actionMasked == MotionEvent.ACTION_UP) v.performClick()
                }
            }
            true
        }
    }

    /**
     * Map a screen-space touch on the SurfaceView into the
     * streamed game-space coordinates the kernel expects. Returns
     * `null` if the touch landed in the letter-box around the
     * scaled framebuffer.
     *
     * The presented picture may be a quarter turn away from the guest's
     * own framebuffer ([rotationDegrees]), in which case the turn has to
     * be undone here or a stylus tap lands somewhere the user did not
     * touch. This is the Android twin of the desktop launcher's
     * `rotated_pointer_to_game`.
     */
    private fun mapTouchToGame(
        v: View,
        event: MotionEvent,
        frame: FrameSnapshot,
    ): Pair<Int, Int>? {
        return displayPointToGuest(event.x,event.y,v.width,v.height,frame.width,frame.height,rotationDegrees,displayScale)
    }

    data class FrameSnapshot(
        val width: Int,
        val height: Int,
        val rgba: ByteArray,
        val rgbaOffset: Int,
    )

    companion object {
        const val EXTRA_GAME_ID = "com.pockethle.app.EXTRA_GAME_ID"
        const val EXTRA_GAME_NAME = "com.pockethle.app.EXTRA_GAME_NAME"

        // Win32 virtual-key codes — same set the desktop GUI uses.
        private const val VK_UP = 0x26
        private const val VK_DOWN = 0x28
        private const val VK_LEFT = 0x25
        private const val VK_RIGHT = 0x27
        private const val VK_RETURN = 0x0D
        private const val VK_SPACE = 0x20
        private const val VK_TAB = 0x09
        private const val VK_SHIFT = 0x10
        private const val VK_CTRL = 0x11
        private const val VK_ESCAPE = 0x1B
        private const val VK_F1 = 0x70
        private const val VK_F2 = 0x71
        private const val VK_F3 = 0x72
        private const val VK_F11 = 0x7A

        /** Polling cadence in ms. 33 ≈ 30 Hz. */
        private const val POLL_INTERVAL_MS = 16L

        /** How far a stick has to travel before it counts as a press.
         * A DualShock 4 rests a few percent off centre. */
        private const val AXIS_DEADZONE = 0.5f

    }
}
