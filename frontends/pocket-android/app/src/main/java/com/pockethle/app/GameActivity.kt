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
    private lateinit var fpsOverlay: TextView
    private val fpsMeter = FrameRateMeter()
    private var showFps = false
    private var activityResumed = false
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
    private var gizmondoGame = true
    private var gameId = ""
    private var gameRoot = ""
    private var pocketPcFullscreen = false
    private var pocketPcFullscreenMode = "with_controls"
    private var dpadDeadzone = ControlDeadzone.LARGE

    /** Android keycode -> guest virtual key currently held. */
    private val heldPhysicalKeys = HashMap<Int, Int>()
    /** Guest virtual keys held by either physical or on-screen controls. */
    private val heldGuestKeys = HashMap<Int, Int>()
    private val heldVirtualKeys = HashSet<Int>()
    /** Guest keys currently asserted by a gamepad stick or hat. */
    private val heldAxisKeys = HashMap<String, Int>()
    private lateinit var stylusBindings: StylusBindings
    private val stylusKeys = HashMap<Pair<Int,Int>,Int>()
    private val stylusAxes = HashMap<Pair<Int,String>,Pair<Int,Float>>()
    private var stylusTouch = floatArrayOf(0f,0f,0f,0f)
    private var stylusSent = 0 to 0
    private var surfacePointerDown = false
    private var surfacePointerId = -1
    private var lastPointerX = 0
    private var lastPointerY = 0

    private val mainHandler = Handler(Looper.getMainLooper())
    private val stylusDeviceListener=object: android.hardware.input.InputManager.InputDeviceListener {
        override fun onInputDeviceAdded(id:Int) {}
        override fun onInputDeviceChanged(id:Int) {}
        override fun onInputDeviceRemoved(id:Int) {
            cancelGrid();gridAxes.clear()
            stylusKeys.keys.removeAll { it.first==id }
            stylusAxes.keys.removeAll { it.first==id }
            updateStylusAim()
        }
    }

    /** Polling tick. ~30 Hz keeps the SurfaceView smooth without
     * burning the CPU on a phone. */
    private var dependencyDialogVisible = false

    private var gridRouter: StylusGridRouter? = null
    private val gridAxes = mutableSetOf<String>()
    private var touchSteeringEnabled = false
    private var steeringDevice = -1
    private val steeringDeviceListener = object : android.hardware.input.InputManager.InputDeviceListener {
        override fun onInputDeviceAdded(deviceId: Int) {}
        override fun onInputDeviceChanged(deviceId: Int) {}
        override fun onInputDeviceRemoved(deviceId: Int) {
            if (deviceId == steeringDevice && session != 0L) {
                NativeBridge.nativeSendInput(session,NativeBridge.INPUT_TOUCH_STEERING,0,0)
                steeringDevice = -1
            }
        }
    }
    private val pollTick = object : Runnable {
        override fun run() {
            if (session == 0L) return
            updateCameraOrientation()
            updateStylusAim()
            val raw = NativeBridge.nativePollFrame(session)
            if (raw != null) {
                decodeFrame(raw)?.let { frame ->
                    lastFrame = frame
                    paintFrame(frame)
                }
            }
            if (!dependencyDialogVisible) {
                NativeBridge.nativePollDependencyNotice(session)?.let { message ->
                    dependencyDialogVisible = true
                    androidx.appcompat.app.AlertDialog.Builder(this@GameActivity)
                        .setTitle("PocketPC dependency").setMessage(message)
                        .setPositiveButton(android.R.string.ok, null)
                        .setOnDismissListener { dependencyDialogVisible = false }.show()
                }
            }
            NativeBridge.nativePollGpsNotice(session)?.let { message ->
                android.widget.Toast.makeText(this@GameActivity, message, android.widget.Toast.LENGTH_LONG).show()
            }
            updateFpsOverlay()
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

    private lateinit var controllerFullscreenHost: ControllerFullscreenHost

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val config = readLauncherConfig()
        bindings = InputBindings(config)
        stylusBindings = StylusBindings(config)
        showFps = config.showFps
        displayScale = config.displayScale
        controlsOpacity = config.controlsOpacity
        gameId = intent.getStringExtra(EXTRA_GAME_ID) ?: ""
        gameRoot = LibraryPaths.root(this)
        gizmondoGame = gameId.isNotEmpty() && NativeBridge.isGizmondoGame(gameRoot,gameId)
        requestedOrientation = if (gizmondoGame) ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE else ActivityInfo.SCREEN_ORIENTATION_FULL_SENSOR
        setContentView(if (gizmondoGame) R.layout.activity_game else R.layout.activity_game_pocketpc)
        val content=findViewById<android.view.ViewGroup>(android.R.id.content)
        val original=content.getChildAt(0)
        content.removeView(original)
        controllerFullscreenHost=ControllerFullscreenHost(this,original) { setControllerFullscreen(false) }
        content.addView(controllerFullscreenHost)
        gameControls = findViewById(R.id.game_controls)
        gameControls.alpha = controlsOpacity
        (gameControls as? PocketPcControls)?.configure(config)
        (gameControls as? GizmondoControls)?.bigDirectionalControls = config.gizmondoControlsSize == "big"
        if (gizmondoGame) {
            androidx.core.view.WindowCompat.setDecorFitsSystemWindows(window,false)
            hideSystemBars()
        } else {
            pocketPcFullscreen=config.fullscreen
            pocketPcFullscreenMode=config.fullscreenMode
            val toolbar=findViewById<androidx.appcompat.widget.Toolbar>(R.id.toolbar)
            setSupportActionBar(toolbar)
            updatePocketPcLayout()
        }

        val name = intent.getStringExtra(EXTRA_GAME_NAME) ?: "PocketHLE"
        title = name

        surface = findViewById(R.id.surface)
        glRenderer = AndroidFrameRenderer(this, config.upscaleFilter, displayScale, showFps, !gizmondoGame)
        findViewById<View>(R.id.btn_fullscreen).setOnClickListener { setControllerFullscreen(true) }
        surface.setEGLContextClientVersion(3)
        surface.setRenderer(glRenderer)
        surface.renderMode = GLSurfaceView.RENDERMODE_WHEN_DIRTY
        progress = findViewById(R.id.progress)
        status = findViewById(R.id.status)
        fpsOverlay = findViewById(R.id.fps_overlay)

        findViewById<View>(R.id.btn_stop_emulation).apply {
            setOnTouchListener { view, event ->
                if (event.actionMasked == MotionEvent.ACTION_DOWN) {
                    controlHaptic(view)
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
        val gameSettings = runCatching { GameSettings.fromJson(JSONObject(NativeBridge.readGameSettings(rootDir, id))) }.getOrDefault(GameSettings.default())
        dpadDeadzone = ControlDeadzone.from(gameSettings.dpadDeadzone)
        touchSteeringEnabled = !gizmondoGame && gameSettings.controlsMode()=="zones"
        gridRouter = if(!gizmondoGame && gameSettings.controlsMode()=="grid") StylusGridRouter(StylusGridConfig.parse(gameSettings.pocketpcStylusGridJson),JSONObject(config.originalJson).optString("gamepad_fullscreen").takeIf { it.isNotEmpty() && it!="null" }) else null
        getSystemService(android.hardware.input.InputManager::class.java)?.registerInputDeviceListener(steeringDeviceListener, mainHandler)
        (gameControls as? GizmondoControls)?.dpad?.deadZoneRadius = dpadDeadzone.radius
        val fixedGps = config.gpsFixedEnabled && NativeBridge.isColorsGame(rootDir, id)
        rotationDegrees = if (isGizmondo) 0 else readRotationDegrees(rootDir, id)
        glRenderer.setRotationDegrees(rotationDegrees)
        if (!gizmondoGame) {
            findViewById<View>(R.id.btn_rotate_game).setOnClickListener {
                releaseHeldInput()
                // The phone sensor owns the layout. This button changes only
                // guest-image presentation, using the same inverse input/SIP map.
                cancelGrid()
                rotationDegrees=(rotationDegrees+90)%360
                glRenderer.setRotationDegrees(rotationDegrees)
                if(session!=0L) NativeBridge.nativeSendInput(session,NativeBridge.INPUT_KEYBOARD_ROTATION,rotationDegrees/90,0)
                val json=JSONObject(NativeBridge.readGameSettings(gameRoot,gameId))
                json.put("rotation",when(rotationDegrees) {90->"cw90";180->"half";270->"ccw90";else->"none"})
                NativeBridge.writeGameSettings(gameRoot,gameId,json.toString())
                lastFrame?.let { frame->updatePocketPcDisplayGeometry(frame);glRenderer.submit(frame);surface.requestRender() }
            }
        }
        BluetoothHost.initialize(this)
        CameraHost.initialize(this)
        updateCameraOrientation()
        GpsHost.initialize(this)
        BatteryHost.initialize(this)
        DeviceStatusHost.initialize(this)
        VibrationHost.initialize(this)
        if (!gizmondoGame && pocketPcFullscreen && pocketPcFullscreenMode=="without_controls") {
            controllerFullscreenHost.setDisplaySize(240,320)
            setControllerFullscreen(true)
        }
        val permissions = (if (config.bluetoothEnabled) BluetoothHost.permissions().toList() else emptyList()) +
            (if (config.cameraEnabled) CameraHost.permissions().toList() else emptyList()) +
            (if (config.gpsEnabled && !fixedGps) GpsHost.permissions().toList() else emptyList()) +
            (if (isGizmondo) DeviceStatusHost.permissions().toList() else emptyList())
        val missing = permissions.distinct().filter { ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED }
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
        if (!gizmondoGame) NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_KEYBOARD_ROTATION, rotationDegrees/90, 0)
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
    @Suppress("DEPRECATION")
    private fun updateCameraOrientation() {
        // Configuration callbacks alone can miss 180-degree landscape changes.
        // Reuse the existing UI poll; CameraHost stores degrees, never an Activity.
        val rotation = windowManager.defaultDisplay.rotation
        val degrees = when (rotation) {
            android.view.Surface.ROTATION_90 -> 90
            android.view.Surface.ROTATION_180 -> 180
            android.view.Surface.ROTATION_270 -> 270
            else -> 0
        }
        CameraHost.setDisplayRotation(if (gizmondoGame) degrees else null)
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        if (!gizmondoGame) releaseHeldInput()
        super.onConfigurationChanged(newConfig)
        updateCameraOrientation()
        if (gizmondoGame) hideSystemBars() else updatePocketPcLayout()
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
        (getSystemService(INPUT_SERVICE) as android.hardware.input.InputManager).registerInputDeviceListener(stylusDeviceListener,mainHandler)
        activityResumed = true
        if (::glRenderer.isInitialized) fpsMeter.reset(glRenderer.presentedFrameCount, System.nanoTime())
        if (::surface.isInitialized) surface.onResume()
        updateCameraOrientation()
        CameraHost.resume()
        GpsHost.resume()
        VibrationHost.resume()
        val handle = session
        if (handle != 0L && !audioRunning) startAudio(handle)
    }

    override fun onPause() {
        (getSystemService(INPUT_SERVICE) as android.hardware.input.InputManager).unregisterInputDeviceListener(stylusDeviceListener)
        activityResumed = false
        if (::fpsOverlay.isInitialized) fpsOverlay.visibility = View.GONE
        if (::surface.isInitialized) surface.onPause()
        CameraHost.pause()
        GpsHost.pause()
        VibrationHost.pause()
        releaseHeldInput()
        stopAudio()
        super.onPause()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus) releaseHeldInput()
        if (hasFocus) hideSystemBars()
    }

    override fun onSupportNavigateUp(): Boolean {
        finish()
        return true
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (joiningSession) return
        if (controllerFullscreenHost.active) { setControllerFullscreen(false); return }
        if (!gizmondoGame && pocketPcFullscreen) {
            releaseHeldInput()
            pocketPcFullscreen=false
            updatePocketPcLayout()
            return
        }

        // Ask the emulator to wind down gracefully; the polling
        // tick will notice the worker exited and call finishSession.
        if (session != 0L) {
            NativeBridge.nativeRequestStop(session)
        }
        if (session != 0L) finishSession() else finish()
    }

    override fun onDestroy() {
        BluetoothHost.closeAll()
        CameraHost.closeAll()
        GpsHost.closeAll()
        VibrationHost.closeAll()
        finishSession()
        mainHandler.removeCallbacksAndMessages(null)
        surface.onPause()
        getSystemService(android.hardware.input.InputManager::class.java)?.unregisterInputDeviceListener(steeringDeviceListener)
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
        fpsOverlay.visibility = View.GONE
        CameraHost.closeAll()
        GpsHost.closeAll()
        VibrationHost.closeAll()
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

    private fun updateFpsOverlay() {
        if (!showFps || !activityResumed) return
        val presentationFps = NativeBridge.nativePresentationFps(session)
        if (presentationFps >= 0f) {
            fpsOverlay.text = String.format(java.util.Locale.US, "%.1f FPS", presentationFps)
            fpsOverlay.visibility = View.VISIBLE
            return
        }
        val total = glRenderer.presentedFrameCount
        if (total == 0L) {
            fpsMeter.reset(0L, System.nanoTime())
            return
        }
        fpsMeter.sample(total, System.nanoTime())?.let { fps ->
            fpsOverlay.text = String.format(java.util.Locale.US, "%.1f IPS", fps)
            fpsOverlay.visibility = View.VISIBLE
        }
    }

    private fun updatePocketPcLayout() {
        if (gizmondoGame) return
        findViewById<View>(R.id.toolbar).visibility=View.GONE
        gameControls.visibility=if (pocketPcFullscreen && pocketPcFullscreenMode=="without_controls") View.GONE else View.VISIBLE
        // Both PPC layouts are immersive game screens. The fullscreen preference
        // only changes the controls policy; it must not reserve a status bar.
        androidx.core.view.WindowCompat.setDecorFitsSystemWindows(window,false)
        hideSystemBars()
        findViewById<View>(R.id.game_root).requestLayout()
    }

    private fun setControllerFullscreen(enabled: Boolean) {
        releaseHeldInput()
        glRenderer.controllerFullscreen=enabled
        if(enabled) {
            val display=findViewById<View>(if(gizmondoGame) R.id.game_area else R.id.content_column)
            controllerFullscreenHost.enter(display)
        } else {
            controllerFullscreenHost.leave()
            pocketPcFullscreen=false
            if(!gizmondoGame) updatePocketPcLayout()
        }
        hideSystemBars()
        lastFrame?.let { frame -> updatePocketPcDisplayGeometry(frame);glRenderer.submit(frame);surface.requestRender() }
    }

    private fun updatePocketPcDisplayGeometry(frame: FrameSnapshot) {
        val swapped=rotationDegrees==90 || rotationDegrees==270
        controllerFullscreenHost.setDisplaySize(if(swapped) frame.height else frame.width,if(swapped) frame.width else frame.height)
        if (gizmondoGame) return
        val quarter = rotationDegrees == 90 || rotationDegrees == 270
        val width = if (quarter) frame.height else frame.width
        val height = if (quarter) frame.width else frame.height
        (findViewById<View>(R.id.game_root) as? PocketPcLayout)?.setDisplaySize(width,height,
            if(displayScale<=0) 4 else displayScale.coerceIn(1,4))

    }

    private fun rotatedDirection(vk:Int):Int {
        if (gizmondoGame) return vk
        return displayDirectionToGuest(vk,rotationDegrees/90)
    }

    private fun paintFrame(frame: FrameSnapshot) {
        updatePocketPcDisplayGeometry(frame)
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
        val gridName=if(isController) gridControllerName(event.keyCode) else gridKeyName(event)
        if(gridName!=null && routeGrid(GridInput(if(isController) "controller" else "keyboard",gridName),event.action==KeyEvent.ACTION_DOWN)) return true
        val steeringDirection=touchSteeringEnabled && (if(isController) gridName?.let{bindings.controllerKey(it)} else bindings.keyboardKey(event)) in listOf(VK_UP,VK_DOWN,VK_LEFT,VK_RIGHT)
        if (!gizmondoGame && !steeringDirection && ::stylusBindings.isInitialized && stylusBindings.enabled && session != 0L) {
            val key=event.deviceId to event.keyCode
            val name=if(isController) InputBindings.controllerName(event.keyCode) else InputBindings.keyName(event)
            val direction=name?.let { if(isController) stylusBindings.pad(it) else stylusBindings.key(it) }
            if(event.action==KeyEvent.ACTION_UP && stylusKeys.remove(key)!=null) { updateStylusAim();return true }
            if(direction!=null) {
                if(event.action==KeyEvent.ACTION_DOWN) stylusKeys[key]=direction
                updateStylusAim();return true
            }
        }
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
        val aimDesired=linkedMapOf<String,Pair<Int,Float>>()
        fun axis(name: String,value: Float,negative: String,positive: String) {
            val steeringArrows=touchSteeringEnabled && name=="DPad"
            if(gridRouter!=null) {
            listOf(name+negative to -value,name+positive to value).forEach { (control,v)->
                val input=GridInput("controller",control)
                if(gridRouter?.assigned(input)==true) {
                    val down=v >= if(control in gridAxes) .14f else .18f
                    if(down) gridAxes.add(control) else gridAxes.remove(control)
                    routeGrid(input,down)
                }
            }
            }
            if(!gizmondoGame && !steeringArrows && stylusBindings.enabled) {
                listOf(name+negative to -value, name+positive to value).forEach { (control,v) ->
                    if(gridRouter?.assigned(GridInput("controller",control))!=true) stylusBindings.pad(control)?.let { direction ->
                        val strength=((v-.18f)/.82f).coerceIn(0f,1f)
                        if(strength>0f) aimDesired[control]=direction to strength
                    }
                }
            }
            fun active(v: Float, control: String) = if (name == "DPad") v >= 0.5f
                else dpadDeadzone.active(v, heldAxisKeys.containsKey(control))
            if(gridRouter?.assigned(GridInput("controller",name+negative))!=true && (steeringArrows || stylusBindings.pad(name+negative).let { gizmondoGame || !stylusBindings.enabled || it==null }) && active(-value, name+negative)) bindings.controllerKey(name+negative)?.let { desired[name+negative]=it }
            if(gridRouter?.assigned(GridInput("controller",name+positive))!=true && (steeringArrows || stylusBindings.pad(name+positive).let { gizmondoGame || !stylusBindings.enabled || it==null }) && active(value, name+positive)) bindings.controllerKey(name+positive)?.let { desired[name+positive]=it }
        }
        axis("DPad",event.getAxisValue(MotionEvent.AXIS_HAT_X),"Left","Right")
        axis("DPad",event.getAxisValue(MotionEvent.AXIS_HAT_Y),"Up","Down")
        if (touchSteeringEnabled) {
            steeringDevice = event.deviceId
            val active = activityResumed && hasWindowFocus() && !surfacePointerDown
            val x = if(active) (event.getAxisValue(MotionEvent.AXIS_X).coerceIn(-1f,1f)*1000).toInt() else 0
            val y = if(active) (event.getAxisValue(MotionEvent.AXIS_Y).coerceIn(-1f,1f)*1000).toInt() else 0
            NativeBridge.nativeSendInput(session,NativeBridge.INPUT_TOUCH_STEERING,x,y)
        } else {
            axis("LeftStick",event.getAxisValue(MotionEvent.AXIS_X),"Left","Right")
            axis("LeftStick",event.getAxisValue(MotionEvent.AXIS_Y),"Up","Down")
        }
        axis("RightStick",event.getAxisValue(MotionEvent.AXIS_Z),"Left","Right")
        axis("RightStick",event.getAxisValue(MotionEvent.AXIS_RZ),"Up","Down")
        listOf("LeftTrigger2" to MotionEvent.AXIS_LTRIGGER,"RightTrigger2" to MotionEvent.AXIS_RTRIGGER).forEach { (name,axis) ->
            val value=event.getAxisValue(axis)
            if(gridRouter?.assigned(GridInput("controller",name))==true) {
                val down=value >= if(name in gridAxes) .14f else .18f
                if(down) gridAxes.add(name) else gridAxes.remove(name)
                routeGrid(GridInput("controller",name),down)
                return@forEach
            }
            val aim=if(!gizmondoGame && stylusBindings.enabled) stylusBindings.pad(name) else null
            if(aim!=null && value>.18f) aimDesired[name]=aim to ((value-.18f)/.82f).coerceIn(0f,1f)
            else if(aim==null && value>.60f) bindings.controllerKey(name)?.let { desired[name]=it }
        }
        stylusAxes.keys.removeAll { it.first==event.deviceId }
        aimDesired.forEach { (name,value) -> stylusAxes[event.deviceId to name]=value }
        updateStylusAim()
        heldAxisKeys.keys.toList().filter { it !in desired }.forEach { name -> heldAxisKeys.remove(name)?.let { releaseGuestKey(handle,it) } }
        desired.forEach { (name,vk) -> if (heldAxisKeys.putIfAbsent(name,vk)==null) acquireGuestKey(handle,vk) }
        return true
    }

    private fun sendGrid(touch:GridTouch) {
        if(session==0L) return
        val frame=lastFrame ?: return
        val (x,y)=StylusGridConfig.guestPoint(touch.x,touch.y,rotationDegrees/90,frame.width,frame.height)
        val kind=when(touch.phase){0->NativeBridge.INPUT_POINTER_DOWN;1->NativeBridge.INPUT_POINTER_MOVE;else->NativeBridge.INPUT_POINTER_UP}
        NativeBridge.nativeSendInput(session,kind,x,y)
    }
    private fun cancelGrid(){gridRouter?.cancel()?.let{sendGrid(it)}}
    private fun routeGrid(input:GridInput,down:Boolean):Boolean {
        val router=gridRouter ?: return false
        if(!router.assigned(input)) return false
        if(!surfacePointerDown && activityResumed && hasWindowFocus()) router.input(input,down)?.let{sendGrid(it)}
        return true
    }

    private fun acquireVirtualKey(handle:Long,vk:Int) {
        if(!gizmondoGame && virtualGridInput(vk)?.let{routeGrid(it,true)}==true)return
        acquireGuestKey(handle,vk)
    }
    private fun releaseVirtualKey(handle:Long,vk:Int) {
        if(!gizmondoGame && virtualGridInput(vk)?.let{routeGrid(it,false)}==true)return
        releaseGuestKey(handle,vk)
    }

    private fun acquireGuestKey(handle: Long, requestedVk: Int) {
        val vk=rotatedDirection(requestedVk)
        val count = heldGuestKeys[vk] ?: 0
        heldGuestKeys[vk] = count + 1
        if (count == 0) {
            NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_KEY_DOWN, vk, 0)
        }
    }

    private fun releaseGuestKey(handle: Long, requestedVk: Int) {
        val vk=rotatedDirection(requestedVk)
        val count = heldGuestKeys[vk] ?: return
        if (count <= 1) {
            heldGuestKeys.remove(vk)
            NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_KEY_UP, vk, 0)
        } else {
            heldGuestKeys[vk] = count - 1
        }
    }

    /** Release keys and pointer state before the native session can outlive the UI. */
    private fun updateStylusAim() {
        if(gizmondoGame || session==0L || !::stylusBindings.isInitialized) return
        val strengths=stylusTouch.copyOf()
        stylusKeys.values.forEach { strengths[it]=1f }
        stylusAxes.values.forEach { (i,v) -> strengths[i]=maxOf(strengths[i],v) }
        val vector=if(!activityResumed || !hasWindowFocus() || surfacePointerDown || gridRouter?.active==true) 0 to 0
            else stylusBindings.velocity(strengths,rotationDegrees/90)
        if(vector!=stylusSent) {
            NativeBridge.nativeSendInput(session,NativeBridge.INPUT_STYLUS_AIM,vector.first,vector.second)
            stylusSent=vector
        }
    }

    private fun releaseHeldInput() {
        cancelGrid()
        gridAxes.clear()
        if(session!=0L && touchSteeringEnabled) NativeBridge.nativeSendInput(session,NativeBridge.INPUT_TOUCH_STEERING,0,0)
        stylusKeys.clear();stylusAxes.clear();stylusTouch.fill(0f)
        if(session!=0L && stylusSent!=(0 to 0)) NativeBridge.nativeSendInput(session,NativeBridge.INPUT_STYLUS_AIM,0,0)
        stylusSent=0 to 0
        (gameControls as? PocketPcControls)?.clearPressedControls()
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
        if (!gizmondoGame) {
            for (id in intArrayOf(R.id.btn_up,R.id.btn_down,R.id.btn_left,R.id.btn_right,
                R.id.btn_action,R.id.btn_turbo,R.id.btn_a,R.id.btn_b,R.id.btn_c,R.id.btn_soft1,R.id.btn_soft2)) {
                findViewById<View?>(id)?.isPressed=false
            }
        }
        heldVirtualKeys.clear()
        heldAxisKeys.clear()
        surfacePointerDown = false
        surfacePointerId = -1
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
            val pointerIndex=if(surfacePointerId>=0) event.findPointerIndex(surfacePointerId) else event.actionIndex
            val mapped = if(pointerIndex>=0) frame?.let { mapTouchToGame(v, event, it,pointerIndex) } else null
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    val (gx, gy) = mapped ?: return@setOnTouchListener true
                    cancelGrid()
                    NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_POINTER_DOWN, gx, gy)
                    surfacePointerDown = true
                    surfacePointerId = event.getPointerId(event.actionIndex)
                    updateStylusAim()
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
                MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP, MotionEvent.ACTION_CANCEL -> {
                    if (surfacePointerDown && (event.actionMasked==MotionEvent.ACTION_CANCEL ||
                            event.getPointerId(event.actionIndex)==surfacePointerId)) {
                        val (gx, gy) = mapped ?: (lastPointerX to lastPointerY)
                        NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_POINTER_UP, gx, gy)
                        surfacePointerDown = false
                        surfacePointerId = -1
                        updateStylusAim()
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
     * Continuous directional pad and independent action buttons. Touch
     * transitions send guest key edges immediately through the same reference
     * counts as hardware input; a diagonal uses two existing directional keys.
     */
    private fun wireVirtualGamepad() {
        (gameControls as? PocketPcControls)?.aim?.apply {
            visibility=if(stylusBindings.enabled || touchSteeringEnabled || gridRouter!=null) View.VISIBLE else View.GONE
            onAim={ x,y ->
                if(touchSteeringEnabled) {
                    NativeBridge.nativeSendInput(session,NativeBridge.INPUT_TOUCH_STEERING,(x*1000).toInt(),(y*1000).toInt())
                } else {
                    val values=floatArrayOf(maxOf(-y,0f),maxOf(y,0f),maxOf(-x,0f),maxOf(x,0f))
                    listOf("Up","Down","Left","Right").forEachIndexed { i,d ->
                        if(routeGrid(GridInput("controller","Touch:Stick$d"),values[i]>.18f))values[i]=0f
                    }
                    stylusTouch=values;updateStylusAim()
                }
            }
        }
        val dpad=(gameControls as? GizmondoControls)?.dpad ?: (gameControls as? PocketPcControls)?.dpad
        dpad?.onDirectionsChanged = { view, mask ->
            val handle = session
            if (handle != 0L) {
                var newlyPressed = false
                val directions = listOf(DpadDirections.UP to VK_UP, DpadDirections.DOWN to VK_DOWN,
                    DpadDirections.LEFT to VK_LEFT, DpadDirections.RIGHT to VK_RIGHT)
                // Release the previous sector before pressing the next one. Other input
                // sources retain their own references to these guest keys.
                directions.forEach { (bit, vk) ->
                    if (mask and bit == 0 && heldVirtualKeys.remove(vk)) releaseVirtualKey(handle, vk)
                }
                directions.forEach { (bit, vk) ->
                    if (mask and bit != 0 && heldVirtualKeys.add(vk)) {
                        acquireVirtualKey(handle, vk)
                        newlyPressed = true
                    }
                }
                if (newlyPressed) controlHaptic(view)
            }
        }
        if (!gizmondoGame) {
            bindVk(R.id.btn_ppc_app1, 0xc1)
            bindVk(R.id.btn_ppc_app2, 0xc2)
            bindVk(R.id.btn_ppc_app3, 0xc3)
            bindVk(R.id.btn_ppc_app4, 0xc4)
            bindVk(R.id.btn_up,VK_UP)
            bindVk(R.id.btn_down,VK_DOWN)
            bindVk(R.id.btn_left,VK_LEFT)
            bindVk(R.id.btn_right,VK_RIGHT)
            bindVk(R.id.btn_turbo,VK_F3)
        }
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

    /** A short, firmer pulse; still respect disabled system/view touch feedback. */
    @Suppress("DEPRECATION")
    private fun controlHaptic(view: View) {
        if (VibrationHost.isGameVibrating()) return
        if (!view.isHapticFeedbackEnabled || android.provider.Settings.System.getInt(
                contentResolver, android.provider.Settings.System.HAPTIC_FEEDBACK_ENABLED, 1) == 0) return
        val vibrator = getSystemService(android.content.Context.VIBRATOR_SERVICE) as? android.os.Vibrator
        val applied = runCatching {
            if (vibrator == null || !vibrator.hasVibrator()) false
            else {
                if (android.os.Build.VERSION.SDK_INT >= 26) {
                    val amplitude = if (vibrator.hasAmplitudeControl()) 180 else android.os.VibrationEffect.DEFAULT_AMPLITUDE
                    vibrator.vibrate(android.os.VibrationEffect.createOneShot(30L, amplitude))
                } else vibrator.vibrate(30L)
                true
            }
        }.getOrDefault(false)
        if (!applied) view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
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
                        acquireVirtualKey(handle, vk)
                        controlHaptic(v)
                    }
                    v.isPressed = true
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    if (heldVirtualKeys.remove(vk)) releaseVirtualKey(handle, vk)
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
        pointerIndex: Int = 0,
    ): Pair<Int, Int>? {
        return displayPointToGuest(event.getX(pointerIndex),event.getY(pointerIndex),v.width,v.height,frame.width,frame.height,rotationDegrees,displayScale,!gizmondoGame,controllerFullscreenHost.active)
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

    }
}
