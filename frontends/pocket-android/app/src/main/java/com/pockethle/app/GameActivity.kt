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
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import android.widget.Toast
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
 * thread roughly every 16 ms, feeds touches and virtual
 * gamepad presses straight back into the kernel, and asks the
 * worker to stop on Back / `onDestroy`.
 */
class GameActivity : AppCompatActivity() {
    private var hardwareStart: (() -> Unit)? = null
    private val hardwarePermissionRequest = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        val start = hardwareStart; hardwareStart = null; start?.invoke()
    }

    private lateinit var surface: GLSurfaceView
    private lateinit var loadingOverlay: View
    private lateinit var status: TextView
    private lateinit var fpsOverlay: TextView
    private val fpsMeter = FrameRateMeter()
    private var frontendFps: Double? = null
    private var showFps = false
    private lateinit var ipsOverlay: TextView
    private var showIps = false
    private var lastIpsUpdateNs = 0L
    private var activityResumed = false
    private lateinit var gameControls: View
    private lateinit var glRenderer: AndroidFrameRenderer
    private lateinit var bindings: InputBindings

    /** Cached handle from `nativeStartGameWithDiagnostic` (`0` once we've finished). */
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
    private var simulateAccelerometerTilt = false
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
    private val stylusPointer=StylusPointer()
    private var stylusPointerTick=0L
    private var stylusCursor: View?=null
    private var surfacePointerDown = false
    private var surfacePointerId = -1
    private var lastPointerX = 0
    private var lastPointerY = 0

    private val mainHandler = Handler(Looper.getMainLooper())
    private val stylusDeviceListener=object: android.hardware.input.InputManager.InputDeviceListener {
        override fun onInputDeviceAdded(id:Int) {}
        override fun onInputDeviceChanged(id:Int) {}
        override fun onInputDeviceRemoved(id:Int) {
            stylusPointer.cancelClick()
            sendGestureEdges(inputGestures.cancelPrefix("pad:$id:"));sendGestureEdges(inputGestures.cancelPrefix("key:$id:"));gestureAxes.removeAll {it.first==id}
            cancelGrid();gridAxes.clear()
            stylusKeys.keys.removeAll { it.first==id }
            stylusAxes.keys.removeAll { it.first==id }
            updateStylusAim()
        }
    }

    /** Prevent multiple dependency prompts during asynchronous session polling. */
    private var dependencyDialogVisible = false

    private var liveGrid: LiveTouchGrid? = null
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
    private var mediaBar: LinearLayout? = null
    private lateinit var mediaProgress: ProgressBar
    private lateinit var mediaButtons: List<TextView>

    private fun mediaCommand(command: Int) {
        if (session != 0L) NativeBridge.nativeSystemMediaCommand(session, command)
    }

    private fun updateMediaBar() {
        val media = NativeBridge.nativeSystemMedia(session)
        val frame = lastFrame
        if (media == null || frame == null || surface.width <= 0 || surface.height <= 0) {
            mediaBar?.visibility = View.GONE
            return
        }
        val status = JSONObject(media)
        val host = findViewById<FrameLayout>(android.R.id.content)
        val bar = mediaBar ?: LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(0xe6000000.toInt())
            isClickable = true
            mediaProgress = ProgressBar(this@GameActivity, null, android.R.attr.progressBarStyleHorizontal)
            mediaProgress.max = 1000
            addView(mediaProgress, LinearLayout.LayoutParams(-1, (4 * resources.displayMetrics.density).toInt()))
            val row = LinearLayout(this@GameActivity)
            mediaButtons = listOf(
                "−10 s" to NativeBridge.MEDIA_REWIND,
                "Play" to NativeBridge.MEDIA_PLAY_PAUSE,
                "Stop" to NativeBridge.MEDIA_STOP,
                "+10 s" to NativeBridge.MEDIA_FORWARD,
            ).map { (label, command) ->
                TextView(this@GameActivity).apply {
                    text = label
                    setTextColor(android.graphics.Color.WHITE)
                    gravity = android.view.Gravity.CENTER
                    isFocusable = true
                    setOnClickListener { mediaCommand(command) }
                    row.addView(this, LinearLayout.LayoutParams(0, -1, 1f))
                }
            }
            addView(row, LinearLayout.LayoutParams(-1, (40 * resources.displayMetrics.density).toInt()))
            host.addView(this, FrameLayout.LayoutParams(0, 0))
            mediaBar = this
        }
        val seekable = !status.isNull("progress")
        mediaProgress.visibility = if (seekable) View.VISIBLE else View.GONE
        if (seekable) mediaProgress.progress = (status.getDouble("progress") * 1000).toInt()
        mediaButtons[0].isEnabled = seekable
        mediaButtons[1].text = if (status.getBoolean("paused")) "Play" else "Pause"
        mediaButtons[3].isEnabled = seekable
        val rect = renderedDisplayRect(
            surface.width, surface.height, frame.width, frame.height, frame.textureWidth,
            frame.textureHeight, rotationDegrees, !gizmondoGame,
            controllerFullscreenHost.active,
        )
        val screen = IntArray(2).also { surface.getLocationInWindow(it) }
        val origin = IntArray(2).also { host.getLocationInWindow(it) }
        val height = ((if (seekable) 44 else 40) * resources.displayMetrics.density).toInt()
        val layout = bar.layoutParams as FrameLayout.LayoutParams
        val left = screen[0] - origin[0] + rect.left
        val top = screen[1] - origin[1] + rect.top + rect.height - height
        if (layout.width != rect.width || layout.height != height ||
            layout.leftMargin != left || layout.topMargin != top) {
            layout.width = rect.width
            layout.height = height
            layout.leftMargin = left
            layout.topMargin = top
            layout.gravity = android.view.Gravity.TOP or android.view.Gravity.LEFT
            bar.layoutParams = layout
        }
        bar.visibility = View.VISIBLE
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
            updateMediaBar()
            updateFpsOverlay()
            updateIpsOverlay()
            if (NativeBridge.nativeIsRunning(session) == 0) {
                // Worker exited on its own (game called ExitProcess
                // / hit max_slices / errored out). Reap it so we
                // surface the summary in the status panel.
                finishSession(reportFailure = true)
                return
            }
            mainHandler.postDelayed(this, POLL_INTERVAL_MS)
        }
    }

    private lateinit var controllerFullscreenHost: ControllerFullscreenHost

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val config = readLauncherConfig()
        stylusBindings = StylusBindings(config)
        showFps = config.showFps
        showIps = config.showIps
        controlsOpacity = config.controlsOpacity
        gameId = intent.getStringExtra(EXTRA_GAME_ID) ?: ""
        gameRoot = LibraryPaths.root(this)
        gizmondoGame = gameId.isNotEmpty() && NativeBridge.isGizmondoGame(gameRoot,gameId)
        bindings = InputBindings(config,gizmondoGame)
        requestedOrientation = if (gizmondoGame) ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE else ActivityInfo.SCREEN_ORIENTATION_FULL_SENSOR
        setContentView(if (gizmondoGame) R.layout.activity_game else R.layout.activity_game_pocketpc)
        val content=findViewById<android.view.ViewGroup>(android.R.id.content)
        val original=content.getChildAt(0)
        content.removeView(original)
        controllerFullscreenHost=ControllerFullscreenHost(this,original) { setControllerFullscreen(false) }
        content.addView(controllerFullscreenHost)
        gameControls = findViewById(R.id.game_controls)
        gameControls.alpha = controlsOpacity
        val launchSettings = runCatching {
            GameSettings.fromJson(JSONObject(NativeBridge.readGameSettings(gameRoot, gameId)))
        }.getOrDefault(GameSettings.default())
        (gameControls as? PocketPcControls)?.configure(config, launchSettings)
        (gameControls as? GizmondoControls)?.apply {
            bigDirectionalControls = config.gizmondoControlsSize == "big"
            maximizeScreen = config.gizmondoMaximizeScreen
        }
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

        val name = intent.getStringExtra(EXTRA_GAME_NAME) ?: "PocketHLE-Reborn"
        title = name

        surface = findViewById(R.id.surface)
        loadingOverlay = findViewById(R.id.game_loading)
        presentationHandle = NativeBridge.nativeCreatePresentation()
        glRenderer = AndroidFrameRenderer(
            this, config.upscaleFilter, showFps, !gizmondoGame, config.graphicsDiagnostics,
            presentationHandle,
            firstFrameDrawn = { mainHandler.post { loadingOverlay.visibility = View.GONE } },
        ) {
            mainHandler.post {
                presentationReady = true
                presentationStart?.let { (root,id) -> presentationStart=null; startSession(root,id) }
            }
        }
        val shareOpenGlContext = config.gpuRenderer ||
            (!gizmondoGame && launchSettings.gpuCompatibilityProfile == "adreno200")
        sharedEglFactory = SharedEglContextFactory(presentationHandle, glRenderer, shareOpenGlContext)
        glRenderer.sharedContextAvailable = { sharedEglFactory.isShared() }
        findViewById<View>(R.id.btn_fullscreen).setOnClickListener { setControllerFullscreen(true) }
        surface.setEGLContextClientVersion(3)
        surface.setEGLContextFactory(sharedEglFactory)
        surface.preserveEGLContextOnPause = shareOpenGlContext
        surface.setRenderer(glRenderer)
        surface.renderMode = GLSurfaceView.RENDERMODE_WHEN_DIRTY
        status = findViewById(R.id.status)
        fpsOverlay = findViewById(R.id.fps_overlay)
        ipsOverlay = findViewById(R.id.ips_overlay)

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
        simulateAccelerometerTilt = !isGizmondo &&
            (gameSettings.pocketpcTiltInput ?: config.pocketpcTiltInput) != TiltInput.ACCELEROMETER
        dpadDeadzone = ControlDeadzone.from(gameSettings.dpadDeadzone)
        touchSteeringEnabled = !gizmondoGame && gameSettings.controlsMode()=="zones"
        gridRouter = if(!gizmondoGame && gameSettings.controlsMode()=="grid") StylusGridRouter(StylusGridConfig.parse(gameSettings.pocketpcStylusGridJson),JSONObject(config.originalJson).optString("gamepad_fullscreen").takeIf { it.isNotEmpty() && it!="null" }) else null
        getSystemService(android.hardware.input.InputManager::class.java)?.registerInputDeviceListener(steeringDeviceListener, mainHandler)
        (gameControls as? GizmondoControls)?.dpad?.deadZoneRadius = dpadDeadzone.radius
        val fixedGps = config.gpsFixedEnabled && NativeBridge.isColorsGame(rootDir, id)
        rotationDegrees = readRotationDegrees(rootDir, id)
        if (!gizmondoGame && intent.getBooleanExtra(EXTRA_CONFIGURE_GRID, false)) {
            gridRouter?.let { router ->
                liveGrid = LiveTouchGrid(
                    this, findViewById(R.id.content_column), findViewById(R.id.game_area),
                    router.config, bindings,
                    JSONObject(config.originalJson).optString("gamepad_fullscreen")
                        .takeIf { it.isNotEmpty() && it != "null" },
                    ::releaseHeldInput,
                ) {
                    runCatching {
                        val raw = JSONObject(NativeBridge.readGameSettings(gameRoot, gameId))
                        check(!raw.has("error"))
                        raw.put("pocketpc_stylus_grid", router.config.json())
                        JSONObject(NativeBridge.writeGameSettings(gameRoot, gameId, raw.toString()))
                            .optBoolean("ok")
                    }.getOrDefault(false)
                }
            }
        }
        glRenderer.setRotationDegrees(rotationDegrees)
        if (!gizmondoGame) {
            findViewById<View>(R.id.btn_rotate_game).setOnClickListener { cycleRotation() }
        }
        BluetoothHost.initialize(this)
        CameraHost.initialize(this)
        updateCameraOrientation()
        GpsHost.initialize(this)
        BatteryHost.initialize(this)
        DeviceStatusHost.initialize(this)
        VibrationHost.initialize(this)
        if (!gizmondoGame && liveGrid == null && pocketPcFullscreen && pocketPcFullscreenMode=="without_controls") {
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

    private var presentationHandle = 0L
    private var presentationReady = false
    private var presentationStart: Pair<String,String>? = null
    private lateinit var sharedEglFactory: SharedEglContextFactory

    private fun startSession(rootDir: String, id: String) {
        if (!presentationReady) { presentationStart = rootDir to id; return }
        if (isFinishing || isDestroyed) return
        if (!simulateAccelerometerTilt) accelerometer.start()
        val startup = runCatching {
            org.json.JSONObject(NativeBridge.nativeStartGameWithDiagnostic(rootDir, id, presentationHandle))
        }.getOrNull()
        val handle = startup?.optLong("handle") ?: 0L
        if (handle == 0L) {
            accelerometer.stop()
            loadingOverlay.visibility = View.GONE
            val diagnostic = startup?.optJSONObject("diagnostic")
            showSessionDiagnostic(
                diagnostic?.optString("title") ?: "Could not start the game",
                diagnostic?.optString("reason") ?: "The native session could not be created.",
                diagnostic?.optString("details").orEmpty()
            )
            return
        }
        session = handle
        if (!gizmondoGame) NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_KEYBOARD_ROTATION, rotationDegrees/90, 0)
        startAudio(handle)
        status.text = "Backend: Unicorn (ARM)\nRunning…"
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

    private val accelerometer by lazy {
        AccelerometerHost(this) {
            @Suppress("DEPRECATION")
            windowManager.defaultDisplay.rotation
        }
    }

    override fun onResume() {
        super.onResume()
        (getSystemService(INPUT_SERVICE) as android.hardware.input.InputManager).registerInputDeviceListener(stylusDeviceListener,mainHandler)
        activityResumed = true
        frontendFps = null
        if (::glRenderer.isInitialized) fpsMeter.reset(glRenderer.presentedFrameCount, System.nanoTime())
        if (::surface.isInitialized) surface.onResume()
        updateCameraOrientation()
        CameraHost.resume()
        GpsHost.resume()
        VibrationHost.resume()
        val handle = session
        if (handle != 0L) {
            if (!simulateAccelerometerTilt) accelerometer.start()
            if (!audioRunning) startAudio(handle)
        }
    }

    override fun onPause() {
        (getSystemService(INPUT_SERVICE) as android.hardware.input.InputManager).unregisterInputDeviceListener(stylusDeviceListener)
        activityResumed = false
        if (::fpsOverlay.isInitialized) fpsOverlay.visibility = View.GONE
        if (::ipsOverlay.isInitialized) ipsOverlay.visibility = View.GONE
        if (::surface.isInitialized) surface.onPause()
        CameraHost.pause()
        GpsHost.pause()
        VibrationHost.pause()
        accelerometer.stop()
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
        if (mediaBar?.visibility == View.VISIBLE) {
            mediaCommand(NativeBridge.MEDIA_STOP)
            return
        }
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
        accelerometer.stop()
        finishSession()
        if (!joiningSession && ::sharedEglFactory.isInitialized) {
            if (::glRenderer.isInitialized) glRenderer.stopPresentation()
            sharedEglFactory.closeAfterJoin()
        }
        mainHandler.removeCallbacksAndMessages(null)
        surface.onPause()
        getSystemService(android.hardware.input.InputManager::class.java)?.unregisterInputDeviceListener(steeringDeviceListener)
        super.onDestroy()
    }

    /**
     * Stop the emulator if it is still running, free the native
     * session, and surface the textual summary in the status panel.
     */
    private fun finishSession(reportFailure: Boolean = false) {
        val handle = session
        if (handle == 0L) return
        joiningSession = true
        glRenderer.stopPresentation()
        fpsOverlay.visibility = View.GONE
        ipsOverlay.visibility = View.GONE
        CameraHost.closeAll()
        GpsHost.closeAll()
        VibrationHost.closeAll()
        accelerometer.stop()
        releaseHeldInput()
        session = 0
        stopAudio()
        val audioToJoin = audioThreads.toList()
        loadingOverlay.visibility = View.GONE
        // `nativeFinishGame` blocks on the worker thread join, so
        // do it off the UI thread to keep the UI responsive — the
        // join is usually fast (the Stop signal already fired) but
        // a long emulator slice can drag it out a few hundred ms.
        Thread {
            NativeBridge.nativeRequestStop(handle)
            audioToJoin.forEach { it.join() }
            val summary = NativeBridge.nativeFinishGame(handle)
            sharedEglFactory.closeAfterJoin()
            mainHandler.post {
                joiningSession = false
                android.util.Log.i("PocketHLE", summary)
                if (!isFinishing && !isDestroyed) {
                    val diagnostic = runCatching {
                        org.json.JSONObject(summary).optJSONObject("diagnostic")
                    }.getOrNull()
                    if (reportFailure && diagnostic != null) {
                        showSessionDiagnostic(
                            diagnostic.getString("title"),
                            diagnostic.getString("reason"),
                            diagnostic.getString("details")
                        )
                    } else if (reportFailure && !summary.startsWith("{")) {
                        showSessionDiagnostic("Emulation stopped", summary, summary)
                    } else {
                        finish()
                    }
                }
            }
        }.start()
    }

    private fun showSessionDiagnostic(title: String, reason: String, details: String) {
        val message = android.widget.TextView(this).apply {
            text = reason
            val padding = (24 * resources.displayMetrics.density).toInt()
            setPadding(padding, padding / 2, padding, padding / 2)
        }
        val scroll = android.widget.ScrollView(this).apply {
            addView(message)
            isFocusable = false
        }
        var expanded = false
        val dialog = AddDataDialog(this) { false }.apply {
            setTitle(title)
            setView(scroll)
            setButton(android.content.DialogInterface.BUTTON_NEUTRAL, "Show details", null as android.content.DialogInterface.OnClickListener?)
            setButton(android.content.DialogInterface.BUTTON_NEGATIVE, "Copy diagnostic", null as android.content.DialogInterface.OnClickListener?)
            setButton(android.content.DialogInterface.BUTTON_POSITIVE, "Return to library") { _, _ -> finish() }
            setCancelable(false)
            controllerCancel = false
        }
        dialog.setOnShowListener {
            val showDetails = dialog.getButton(android.content.DialogInterface.BUTTON_NEUTRAL)
            showDetails.setOnClickListener {
                expanded = !expanded
                message.text = if (expanded) "$reason\n\n$details" else reason
                scroll.isFocusable = expanded
                showDetails.text = if (expanded) "Hide details" else "Show details"
            }
            dialog.getButton(android.content.DialogInterface.BUTTON_NEGATIVE).setOnClickListener {
                val clipboard = getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
                clipboard.setPrimaryClip(android.content.ClipData.newPlainText("PocketHLE-Reborn diagnostic", "$title\n$reason\n\n$details"))
            }
            showDetails.requestFocus()
        }
        dialog.show()
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
        val frame = decodeFramePayload(raw) ?: return null
        return FrameSnapshot(frame.width, frame.height, raw, frame.rgbaOffset, frame.textureWidth, frame.textureHeight, frame.sharedToken, frame.rgb565, frame.backlight)
    }

    private fun updateIpsOverlay() {
        if (!showIps || !activityResumed || session == 0L) return
        val now = System.nanoTime()
        if (now - lastIpsUpdateNs < 1_000_000_000L) return
        lastIpsUpdateNs = now
        val ips = NativeBridge.nativePublishedImageIps(session).coerceAtLeast(0f)
        val fps = frontendFps
        ipsOverlay.text = if (fps != null) {
            String.format(java.util.Locale.US, "%.0f FPS / %.0f IPS", fps, ips)
        } else {
            String.format(java.util.Locale.US, "%.0f IPS", ips)
        }
        ipsOverlay.visibility = View.VISIBLE
    }

    private fun updateFpsOverlay() {
        if (!showFps || !activityResumed) return
        val total = glRenderer.presentedFrameCount
        if (total == 0L) {
            fpsMeter.reset(0L, System.nanoTime())
            return
        }
        fpsMeter.sample(total, System.nanoTime())?.let { fps ->
            frontendFps = fps
            if (!showIps) {
                fpsOverlay.text = String.format(java.util.Locale.US, "%.1f FPS", fps)
                fpsOverlay.visibility = View.VISIBLE
            }
        }
    }

    private fun updatePocketPcLayout() {
        if (gizmondoGame) return
        findViewById<View>(R.id.toolbar).visibility=View.GONE
        gameControls.visibility=if (liveGrid == null && pocketPcFullscreen && pocketPcFullscreenMode=="without_controls") View.GONE else View.VISIBLE
        // Both PPC layouts are immersive game screens. The fullscreen preference
        // only changes the controls policy; it must not reserve a status bar.
        androidx.core.view.WindowCompat.setDecorFitsSystemWindows(window,false)
        hideSystemBars()
        findViewById<View>(R.id.game_root).requestLayout()
    }

    private fun setControllerFullscreen(enabled: Boolean) {
        if (enabled && liveGrid != null) return
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
        val quarter=rotationDegrees==90 || rotationDegrees==270
        val renderW=if(quarter) frame.textureHeight else frame.textureWidth
        val renderH=if(quarter) frame.textureWidth else frame.textureHeight
        controllerFullscreenHost.setDisplaySize(renderW,renderH)
        if (gizmondoGame) return
        val width=if(quarter) frame.height else frame.width
        val height=if(quarter) frame.width else frame.height
        (findViewById<View>(R.id.game_root) as? PocketPcLayout)?.setDisplaySize(width,height,renderW,renderH)
    }

    private fun rotatedDirection(vk:Int):Int {
        if (gizmondoGame) return vk
        return displayDirectionToGuest(vk,rotationDegrees/90)
    }

    private fun paintFrame(frame: FrameSnapshot) {
        updatePocketPcDisplayGeometry(frame)
        liveGrid?.let { grid ->
            val rect = renderedDisplayRect(surface.width, surface.height, frame.width, frame.height,
                frame.textureWidth, frame.textureHeight, rotationDegrees, true, controllerFullscreenHost.active)
            grid.updateDisplay(rect, rect.height > rect.width)
        }
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
     * Counts are kept per guest key because several physical sources can
     * share a guest virtual-key code.
     */
    private val deckHosts=DeckHostTracker()
    private val hostLongCheck=Runnable {
        deckHosts.due(android.os.SystemClock.uptimeMillis(),gizmondoGame).forEach {performHostShortcut(it)}
    }
    private fun cycleRotation() {
        if (!::glRenderer.isInitialized || lastFrame == null) return
        releaseHeldInput()
        cancelGrid()
        rotationDegrees=(rotationDegrees+90)%360
        glRenderer.setRotationDegrees(rotationDegrees)
        if(session!=0L) NativeBridge.nativeSendInput(session,NativeBridge.INPUT_KEYBOARD_ROTATION,rotationDegrees/90,0)
        runCatching {
            val json=JSONObject(NativeBridge.readGameSettings(gameRoot,gameId))
            json.put("rotation",when(rotationDegrees) {90->"cw90";180->"half";270->"ccw90";else->"none"})
            NativeBridge.writeGameSettings(gameRoot,gameId,json.toString())
        }
        lastFrame?.let { frame->updatePocketPcDisplayGeometry(frame);glRenderer.submit(frame);surface.requestRender() }
    }

    private fun performHostShortcut(action:DeckInputPolicy.Action) {
        when(action) {
            DeckInputPolicy.Action.ROTATION -> cycleRotation()
            DeckInputPolicy.Action.FULLSCREEN -> setControllerFullscreen(!glRenderer.controllerFullscreen)
            DeckInputPolicy.Action.EXIT -> finish()
            DeckInputPolicy.Action.NEXT_FILTER -> surface.queueEvent {
                val filter=glRenderer.cycleFilter();surface.requestRender()
                mainHandler.post {
                    runCatching {val config=JSONObject(NativeBridge.readConfig(gameRoot));config.put("upscale_filter",filter);NativeBridge.writeConfig(gameRoot,config.toString())}
                    Toast.makeText(this,"Filter: $filter",Toast.LENGTH_SHORT).show()
                }
            }
        }
    }
    private val inputGestures = InputGestures()
    private val gestureAxes = mutableSetOf<Pair<Int, String>>()

    private fun sendGestureEdges(edges: List<Pair<Int, Boolean>>) {
        val handle = session
        if (handle == 0L) return
        edges.forEach { (vk, down) ->
            val mapped = mappedPhysicalKey(vk)
            if (down) acquireGuestKey(handle, mapped) else releaseGuestKey(handle, mapped)
        }
    }

    private val gestureTick =
        object : Runnable {
            override fun run() {
                if (session == 0L || !activityResumed || !hasWindowFocus()) return
                sendGestureEdges(inputGestures.tick(android.os.SystemClock.uptimeMillis()))
                if (inputGestures.active()) mainHandler.postDelayed(this, 10)
            }
        }

    private fun routeLongPress(
        device: Int,
        pad: Boolean,
        input: String,
        down: Boolean,
        channel: String = "key",
    ): Boolean {
        if (!::bindings.isInitialized || session == 0L) return false
        val long = bindings.longKey(input, pad)
        if (long == null && !bindings.hasShort(input, pad)) return false
        val short = if (pad) bindings.controllerKey(input) else bindings.keyboardName(input)
        val token =
            "${if(pad) "pad" else "key"}:$device:$channel:${InputBindings.controllerCanonical(input)}"
        sendGestureEdges(
            if (long != null)
                inputGestures.edge(token, down, android.os.SystemClock.uptimeMillis(), short, long)
            else inputGestures.immediateEdge(token, down, short)
        )
        mainHandler.removeCallbacks(gestureTick)
        if (inputGestures.active()) mainHandler.postDelayed(gestureTick, 10)
        return true
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        liveGrid?.let { grid ->
            if (grid.key(event)) return true
            if (grid.editing) return super.dispatchKeyEvent(event)
        }
        val keyAction=InputBindings.keyName(event)?.let {DeckInputPolicy.key(it)}
        if(keyAction!=null) {
            if(event.action==KeyEvent.ACTION_DOWN && event.repeatCount==0 && ::glRenderer.isInitialized) performHostShortcut(keyAction)
            return true
        }
        if (InputBindings.keyName(event)?.let { DeckInputPolicy.reservedKey(it) } == true) return true
        if(event.keyCode==KeyEvent.KEYCODE_ESCAPE && ::glRenderer.isInitialized && glRenderer.controllerFullscreen) {
            if(event.action==KeyEvent.ACTION_DOWN && event.repeatCount==0) setControllerFullscreen(false)
            return true
        }
        val sources = event.device?.sources ?: event.source
        val isController = sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD || sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
        val controllerName=if(isController) InputBindings.controllerName(event.keyCode) else null
        if(controllerName=="RightThumb" && !gizmondoGame) {
            if(event.repeatCount==0 && session!=0L && activityResumed && hasWindowFocus()) {
                stylusButton(event.action==KeyEvent.ACTION_DOWN)
            }
            return true
        }
        if(controllerName!=null && DeckInputPolicy.reserved(controllerName,gizmondoGame)) {
            val now=android.os.SystemClock.uptimeMillis()
            if(event.action==KeyEvent.ACTION_DOWN && event.repeatCount==0) {
                deckHosts.down(event.deviceId,controllerName,now,gizmondoGame)
                mainHandler.postDelayed(hostLongCheck,DeckInputPolicy.HOLD_MS)
            } else if(event.action==KeyEvent.ACTION_UP) deckHosts.up(event.deviceId,controllerName,now,gizmondoGame)?.let {performHostShortcut(it)}
            return true
        }
        val gestureName=if(isController) controllerName else InputBindings.keyName(event)
        if(gestureName!=null && routeLongPress(event.deviceId,isController,gestureName,event.action==KeyEvent.ACTION_DOWN)) return true
        val gridName=if(isController) gridControllerName(event.keyCode) else gridKeyName(event)
        if(gridName!=null && routeGrid(GridInput(if(isController) "controller" else "keyboard",gridName),event.action==KeyEvent.ACTION_DOWN)) return true
        val steeringDirection=touchSteeringEnabled && (if(isController) gridName?.let{bindings.controllerKey(it)} else bindings.keyboardKey(event)) in listOf(VK_UP,VK_DOWN,VK_LEFT,VK_RIGHT)
        if (!gizmondoGame && !steeringDirection && (!isController || controllerName==null || DeckInputPolicy.guest(controllerName,gizmondoGame)==null) && ::stylusBindings.isInitialized && session != 0L) {
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
        val physicalVk = mappedPhysicalKey(vk)
        val handle = session
        if (handle == 0L) return super.dispatchKeyEvent(event)
        when (event.action) {
            KeyEvent.ACTION_DOWN -> {
                if (event.repeatCount == 0 && heldPhysicalKeys.putIfAbsent(event.keyCode, physicalVk) == null) {
                    acquireGuestKey(handle, physicalVk)
                }
            }
            KeyEvent.ACTION_UP -> {
                heldPhysicalKeys.remove(event.keyCode)?.let { releaseGuestKey(handle, it) }
            }
        }
        return true
    }

    private fun mappedPhysicalKey(vk: Int): Int {
        val controls=gameControls as? PocketPcControls ?: return vk
        return pocketPcPhysicalKey(vk, controls.swapButtonGroups, controls.controllerLayout)
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
        if (liveGrid?.motion(event) == true) return true
        val joystick =
            event.source and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK ||
                event.source and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD
        val handle = session
        if (!joystick || handle == 0L || event.action != MotionEvent.ACTION_MOVE) {
            return super.onGenericMotionEvent(event)
        }
        val gestureControls = mutableSetOf<String>()
        fun gestureAxis(control: String, strength: Float) {
            if(DeckInputPolicy.stylusReserved(control,gizmondoGame)) return
            if (bindings.longKey(control, true) == null && !bindings.hasShort(control, true)) return
            gestureControls.add(control)
            val token = event.deviceId to control
            val down = strength.isFinite() && strength >= if (token in gestureAxes) .15f else .25f
            if (down != (token in gestureAxes)) {
                if (down) gestureAxes.add(token) else gestureAxes.remove(token)
                routeLongPress(event.deviceId, true, control, down, "axis")
            }
        }
        fun gesturePair(axis: Int, negative: String, positive: String) {
            val value = event.getAxisValue(axis)
            gestureAxis(negative, -value)
            gestureAxis(positive, value)
        }
        gesturePair(MotionEvent.AXIS_HAT_X, "DPadLeft", "DPadRight")
        gesturePair(MotionEvent.AXIS_HAT_Y, "DPadUp", "DPadDown")
        gesturePair(MotionEvent.AXIS_X, "LeftStickLeft", "LeftStickRight")
        gesturePair(MotionEvent.AXIS_Y, "LeftStickUp", "LeftStickDown")
        gesturePair(MotionEvent.AXIS_Z, "RightStickLeft", "RightStickRight")
        gesturePair(MotionEvent.AXIS_RZ, "RightStickUp", "RightStickDown")
        if (liveGrid == null) gestureAxis("LeftTrigger2", event.getAxisValue(MotionEvent.AXIS_LTRIGGER))
        if (liveGrid == null) gestureAxis("RightTrigger2", event.getAxisValue(MotionEvent.AXIS_RTRIGGER))
        val desired = linkedMapOf<String, Int>()
        val aimDesired = linkedMapOf<String, Pair<Int, Float>>()
        fun axis(name: String, value: Float, negative: String, positive: String) {
            val steeringArrows = touchSteeringEnabled && name == "DPad"
            if (gridRouter != null) {
                listOf(name + negative to -value, name + positive to value).forEach { (control, v)
                    ->
                    val input = GridInput("controller", control)
                    if (control !in gestureControls && gridRouter?.assigned(input) == true) {
                        val down = v >= if (control in gridAxes) .14f else .18f
                        if (down) gridAxes.add(control) else gridAxes.remove(control)
                        routeGrid(input, down)
                    }
                }
            }
            if (!gizmondoGame && !steeringArrows) {
                listOf(name + negative to -value, name + positive to value).forEach { (control, v)
                    ->
                    if (
                        control !in gestureControls &&
                            (DeckInputPolicy.stylusReserved(control,gizmondoGame) || gridRouter?.assigned(GridInput("controller", control)) != true)
                    )
                        stylusBindings.pad(control)?.let { direction ->
                            val strength = ((v - .18f) / .82f).coerceIn(0f, 1f)
                            if (strength > 0f) aimDesired[control] = direction to strength
                        }
                }
            }
            fun active(v: Float, control: String) =
                if (name == "DPad") v >= 0.5f
                else dpadDeadzone.active(v, heldAxisKeys.containsKey(control))
            if (
                name + negative !in gestureControls &&
                    gridRouter?.assigned(GridInput("controller", name + negative)) != true &&
                    (steeringArrows ||
                        stylusBindings.pad(name + negative).let {
                            gizmondoGame || it == null
                        }) &&
                    active(-value, name + negative)
            )
                bindings.controllerKey(name + negative)?.let { desired[name + negative] = it }
            if (
                name + positive !in gestureControls &&
                    gridRouter?.assigned(GridInput("controller", name + positive)) != true &&
                    (steeringArrows ||
                        stylusBindings.pad(name + positive).let {
                            gizmondoGame || it == null
                        }) &&
                    active(value, name + positive)
            )
                bindings.controllerKey(name + positive)?.let { desired[name + positive] = it }
        }
        axis("DPad", event.getAxisValue(MotionEvent.AXIS_HAT_X), "Left", "Right")
        axis("DPad", event.getAxisValue(MotionEvent.AXIS_HAT_Y), "Up", "Down")
        if (touchSteeringEnabled || simulateAccelerometerTilt) {
            steeringDevice = event.deviceId
            val active = activityResumed && hasWindowFocus() && (simulateAccelerometerTilt || !surfacePointerDown)
            val x =
                if (active)
                    (event.getAxisValue(MotionEvent.AXIS_X).coerceIn(-1f, 1f) * 1000).toInt()
                else 0
            val y =
                if (active)
                    (event.getAxisValue(MotionEvent.AXIS_Y).coerceIn(-1f, 1f) * 1000).toInt()
                else 0
            NativeBridge.nativeSendInput(session, NativeBridge.INPUT_TOUCH_STEERING, x, y)
        } else {
            axis("LeftStick", event.getAxisValue(MotionEvent.AXIS_X), "Left", "Right")
            axis("LeftStick", event.getAxisValue(MotionEvent.AXIS_Y), "Up", "Down")
        }
        axis("RightStick", event.getAxisValue(MotionEvent.AXIS_Z), "Left", "Right")
        axis("RightStick", event.getAxisValue(MotionEvent.AXIS_RZ), "Up", "Down")
        if (liveGrid == null) listOf(
                "LeftTrigger2" to MotionEvent.AXIS_LTRIGGER,
                "RightTrigger2" to MotionEvent.AXIS_RTRIGGER,
            )
            .forEach { (name, axis) ->
                val value = event.getAxisValue(axis)
                if (name in gestureControls) return@forEach
                if (gridRouter?.assigned(GridInput("controller", name)) == true) {
                    val down = value >= if (name in gridAxes) .14f else .18f
                    if (down) gridAxes.add(name) else gridAxes.remove(name)
                    routeGrid(GridInput("controller", name), down)
                    return@forEach
                }
                val aim =
                    if (
                        !gizmondoGame &&
                            DeckInputPolicy.guest(name, gizmondoGame) == null
                    )
                        stylusBindings.pad(name)
                    else null
                if (aim != null && value > .18f)
                    aimDesired[name] = aim to ((value - .18f) / .82f).coerceIn(0f, 1f)
                else if (aim == null && value > .60f)
                    bindings.controllerKey(name)?.let { desired[name] = it }
            }
        stylusAxes.keys.removeAll { it.first == event.deviceId }
        aimDesired.forEach { (name, value) -> stylusAxes[event.deviceId to name] = value }
        updateStylusAim()
        heldAxisKeys.keys
            .toList()
            .filter { it !in desired }
            .forEach { name -> heldAxisKeys.remove(name)?.let { releaseGuestKey(handle, it) } }
        desired.forEach { (name, vk) ->
            val mapped = mappedPhysicalKey(vk)
            if (heldAxisKeys.putIfAbsent(name, mapped) == null) acquireGuestKey(handle, mapped)
        }
        return true
    }

    private fun sendGrid(touch:GridTouch) {
        if (touch.phase == 0) { stylusPointer.cancelClick();cancelCenterTap() }
        if(session==0L) return
        val frame=lastFrame ?: return
        val (x,y)=StylusGridConfig.guestPoint(touch.x,touch.y,rotationDegrees/90,frame.width,frame.height)
        val kind=when(touch.phase){0->NativeBridge.INPUT_POINTER_DOWN;1->NativeBridge.INPUT_POINTER_MOVE;else->NativeBridge.INPUT_POINTER_UP}
        NativeBridge.nativeSendInput(session,kind,x,y)
    }
    private fun cancelGrid(){gridRouter?.cancel()?.let{sendGrid(it)}}
    private fun routeGrid(input:GridInput,down:Boolean):Boolean {
        if (liveGrid?.virtualInput(input, down) == true) return true
        if(input.source=="keyboard" && bindings.stylusKeyboardReserved(input.name)) return false
        val router=gridRouter ?: return false
        if(!router.assigned(input)) return false
        if (!surfacePointerDown && activityResumed && hasWindowFocus()) {
            lastFrame?.let { frame ->
                val quarterTurn = rotationDegrees == 90 || rotationDegrees == 270
                router.config.orient((frame.height > frame.width) xor quarterTurn)
            }
            router.input(input, down)?.let { sendGrid(it) }
        }
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

    private val centerTap = CenterTapPulse()
    private var centerTapSession = 0L
    private val centerTapRelease = object : Runnable {
        override fun run() {
            val point = centerTap.releaseDue(android.os.SystemClock.uptimeMillis())
            if (point != null) {
                if (session != 0L && session == centerTapSession)
                    NativeBridge.nativeSendInput(session,NativeBridge.INPUT_POINTER_UP,point.first,point.second)
                centerTapSession = 0L
                updateStylusAim()
            } else if (centerTap.active) mainHandler.postDelayed(this,1)
        }
    }

    private fun cancelCenterTap() {
        mainHandler.removeCallbacks(centerTapRelease)
        val point = centerTap.cancel()
        if (point != null && session != 0L && session == centerTapSession)
            NativeBridge.nativeSendInput(session,NativeBridge.INPUT_POINTER_UP,point.first,point.second)
        centerTapSession = 0L
        if (point != null) updateStylusAim()
    }

    private fun tapScreenCenter(handle: Long) {
        if (gizmondoGame || handle == 0L || handle != session || surfacePointerDown || gridRouter?.active == true) return
        val frame = lastFrame ?: return
        val point = centerTap.press(android.os.SystemClock.uptimeMillis(),frame.width,frame.height) ?: return
        centerTapSession = handle
        updateStylusAim()
        NativeBridge.nativeSendInput(handle,NativeBridge.INPUT_POINTER_DOWN,point.first,point.second)
        mainHandler.postDelayed(centerTapRelease,CenterTapPulse.DURATION_MS)
    }

    private fun stylusButton(down:Boolean) {
        if(gizmondoGame || session==0L || !activityResumed || !hasWindowFocus()) return
        if(stylusPointer.button(down,android.os.SystemClock.uptimeMillis())) {
            updateStylusAim()
            Toast.makeText(this,"Stylus mode: ${if(stylusPointer.pointer) "Pointer" else "Stick"}",Toast.LENGTH_SHORT).show()
        }
    }

    private fun acquireGuestKey(handle: Long, requestedVk: Int) {
        if (liveGrid?.editing == true) return
        if(requestedVk==DeckInputPolicy.STYLUS_CLICK_COMMAND) {if(handle==session) stylusButton(true);return}
        if (requestedVk == DeckInputPolicy.ROTATION_COMMAND) { if (handle == session) cycleRotation(); return }
        if (requestedVk == DeckInputPolicy.SCREENSHOT_COMMAND) return
        if (requestedVk == CenterTapPulse.COMMAND) { tapScreenCenter(handle); return }
        val vk=rotatedDirection(requestedVk)
        val count = heldGuestKeys[vk] ?: 0
        heldGuestKeys[vk] = count + 1
        if (count == 0) {
            NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_KEY_DOWN, vk, 0)
        }
    }

    private fun releaseGuestKey(handle: Long, requestedVk: Int) {
        if(requestedVk==DeckInputPolicy.STYLUS_CLICK_COMMAND) {if(handle==session) stylusButton(false);return}
        if (requestedVk == CenterTapPulse.COMMAND || requestedVk == DeckInputPolicy.ROTATION_COMMAND || requestedVk == DeckInputPolicy.SCREENSHOT_COMMAND) return
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
    private fun updateStylusCursor() {
        if(!::surface.isInitialized || (!stylusPointer.pointer && stylusCursor==null)) return
        val overlay=stylusCursor ?: object: View(this) {
            private val paint=android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG)
            override fun onDraw(canvas:android.graphics.Canvas) {
                val f=lastFrame ?: return
                val p=stylusPointer.position() ?: return
                if(!stylusPointer.pointer || gizmondoGame) return
                val rect=renderedDisplayRect(width,height,f.width,f.height,f.textureWidth,f.textureHeight,rotationDegrees,true,controllerFullscreenHost.active)
                val w=f.width.toFloat();val h=f.height.toFloat();val x=p.first.toFloat();val y=p.second.toFloat()
                val uv=when(rotationDegrees) {90->(h-1-y)/h to x/w;180->(w-1-x)/w to (h-1-y)/h;270->y/h to (w-1-x)/w;else->x/w to y/h}
                val cx=rect.left+uv.first*rect.width;val cy=rect.top+uv.second*rect.height
                val radius=7*resources.displayMetrics.density
                canvas.save();canvas.clipRect(rect.left,rect.top,rect.left+rect.width,rect.top+rect.height)
                listOf(4f to android.graphics.Color.BLACK,2f to android.graphics.Color.WHITE).forEach { (stroke,color)->
                    paint.color=color;paint.strokeWidth=stroke*resources.displayMetrics.density
                    canvas.drawLine(cx-radius,cy,cx+radius,cy,paint);canvas.drawLine(cx,cy-radius,cx,cy+radius,paint)
                };canvas.restore()
            }
        }.also {
            it.isClickable=false;it.isFocusable=false
            (surface.parent as android.widget.FrameLayout).addView(it,android.widget.FrameLayout.LayoutParams(-1,-1))
            stylusCursor=it
        }
        overlay.visibility=if(stylusPointer.pointer) View.VISIBLE else View.GONE
        if(stylusPointer.pointer) overlay.invalidate()
    }

    private fun updateStylusAim() {
        if (liveGrid?.editing == true) return
        if(gizmondoGame || session==0L || !::stylusBindings.isInitialized) return
        val strengths=stylusTouch.copyOf()
        stylusKeys.values.forEach { strengths[it]=1f }
        stylusAxes.values.forEach { (i,v) -> strengths[i]=maxOf(strengths[i],v) }
        var vector=if(!activityResumed || !hasWindowFocus() || surfacePointerDown || centerTap.active || gridRouter?.active==true) 0 to 0
            else lastFrame?.let { stylusBindings.velocityForSize(strengths,rotationDegrees/90,it.width,it.height) } ?: (0 to 0)
        val now=android.os.SystemClock.uptimeMillis()
        val active=activityResumed && hasWindowFocus() && !surfacePointerDown && gridRouter?.active!=true
        lastFrame?.let { frame -> stylusPointer.advance(vector,if(stylusPointerTick==0L) 0f else (now-stylusPointerTick)/1000f,frame.width,frame.height) }
        stylusPointerTick=now
        if(!active) stylusPointer.cancelClick()
        val tap=stylusPointer.takeTap(now)
        if(active && tap!=null && !centerTap.active) {
            centerTap.pressAt(now,tap.first,tap.second)
            centerTapSession=session
            NativeBridge.nativeSendInput(session,NativeBridge.INPUT_STYLUS_AIM,0,0)
            NativeBridge.nativeSendInput(session,NativeBridge.INPUT_POINTER_DOWN,tap.first,tap.second)
            mainHandler.postDelayed(centerTapRelease,CenterTapPulse.DURATION_MS)
        }
        if(stylusPointer.pointer) vector=0 to 0
        updateStylusCursor()
        if(vector!=stylusSent) {
            NativeBridge.nativeSendInput(session,NativeBridge.INPUT_STYLUS_AIM,vector.first,vector.second)
            stylusSent=vector
        }
    }

    private fun releaseHeldInput() {
        stylusPointer.cancelClick()
        stylusPointerTick=0L
        cancelCenterTap()
        inputGestures.clear()
        gestureAxes.clear()
        mainHandler.removeCallbacks(gestureTick)
        deckHosts.clear()
        mainHandler.removeCallbacks(hostLongCheck)
        cancelGrid()
        gridAxes.clear()
        if (session != 0L && (touchSteeringEnabled || simulateAccelerometerTilt))
            NativeBridge.nativeSendInput(session, NativeBridge.INPUT_TOUCH_STEERING, 0, 0)
        stylusKeys.clear()
        stylusAxes.clear()
        stylusTouch.fill(0f)
        if (session != 0L && stylusSent != (0 to 0))
            NativeBridge.nativeSendInput(session, NativeBridge.INPUT_STYLUS_AIM, 0, 0)
        stylusSent = 0 to 0
        (gameControls as? PocketPcControls)?.clearPressedControls()
        val handle = session
        if (handle != 0L) {
            for (vk in heldGuestKeys.keys.toList()) {
                NativeBridge.nativeSendInput(handle, NativeBridge.INPUT_KEY_UP, vk, 0)
            }
            if (surfacePointerDown) {
                NativeBridge.nativeSendInput(
                    handle,
                    NativeBridge.INPUT_POINTER_UP,
                    lastPointerX,
                    lastPointerY,
                )
            }
        }
        heldPhysicalKeys.clear()
        heldGuestKeys.clear()
        (gameControls as? GizmondoControls)?.clearPressedControls()
        if (!gizmondoGame) {
            for (id in
                intArrayOf(
                    R.id.btn_up,
                    R.id.btn_down,
                    R.id.btn_left,
                    R.id.btn_right,
                    R.id.btn_action,
                    R.id.btn_turbo,
                    R.id.btn_a,
                    R.id.btn_b,
                    R.id.btn_c,
                    R.id.btn_soft1,
                    R.id.btn_soft2,
                )) {
                findViewById<View?>(id)?.isPressed = false
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
            if (liveGrid?.editing == true) return@setOnTouchListener true
            val handle = session
            if (handle == 0L) return@setOnTouchListener false
            val frame = lastFrame
            val pointerIndex=if(surfacePointerId>=0) event.findPointerIndex(surfacePointerId) else event.actionIndex
            val mapped = if(pointerIndex>=0) frame?.let { mapTouchToGame(v, event, it,pointerIndex) } else null
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    stylusPointer.cancelClick()
                    cancelCenterTap()
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
            visibility=View.VISIBLE
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
        return displayPointToGuest(event.getX(pointerIndex),event.getY(pointerIndex),v.width,v.height,frame.width,frame.height,rotationDegrees,!gizmondoGame,controllerFullscreenHost.active,frame.textureWidth,frame.textureHeight)
    }

    data class FrameSnapshot(
        val width: Int,
        val height: Int,
        val rgba: ByteArray,
        val rgbaOffset: Int,
        val textureWidth: Int = width,
        val textureHeight: Int = height,
        val sharedToken: Long = 0,
        val rgb565: Boolean = false,
        val backlight: Int = 11,
    )

    companion object {
        const val EXTRA_CONFIGURE_GRID = "configure_grid"
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

        /** UI polling cadence; this does not measure displayed FPS. */
        private const val POLL_INTERVAL_MS = 16L

    }
}
