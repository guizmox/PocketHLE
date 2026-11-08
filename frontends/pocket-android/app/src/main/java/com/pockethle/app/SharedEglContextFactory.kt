package com.pockethle.app

import android.opengl.GLSurfaceView
import android.util.Log
import javax.microedition.khronos.egl.*

/** Root is independent of the replaceable SurfaceView context. Native retains
 * Android's ref-counted EGL display. Close only after all emulator jobs join.
 */
internal class SharedEglContextFactory(private val id: Long, private val renderer: AndroidFrameRenderer, private val requested: Boolean = true) : GLSurfaceView.EGLContextFactory {
    private var root: EGLContext = EGL10.EGL_NO_CONTEXT
    private var rootSurface: EGLSurface = EGL10.EGL_NO_SURFACE
    private var rootDisplay: EGLDisplay = EGL10.EGL_NO_DISPLAY
    private var api: EGL10? = null
    private var closing = false
    private var sharedUi = false

    @Synchronized override fun createContext(egl: EGL10, display: EGLDisplay, config: EGLConfig): EGLContext {
        if (closing) { sharedUi=false; return egl.eglCreateContext(display,config,EGL10.EGL_NO_CONTEXT,intArrayOf(0x3098,3,EGL10.EGL_NONE)) }
        api = egl
        if (!requested) return egl.eglCreateContext(display,config,EGL10.EGL_NO_CONTEXT,intArrayOf(0x3098,3,EGL10.EGL_NONE))
        if (root == EGL10.EGL_NO_CONTEXT) {
            val configs = arrayOfNulls<EGLConfig>(1)
            val count = IntArray(1)
            val rootConfig = if (egl.eglChooseConfig(display,intArrayOf(EGL10.EGL_SURFACE_TYPE,EGL10.EGL_PBUFFER_BIT,EGL10.EGL_RENDERABLE_TYPE,0x40,EGL10.EGL_RED_SIZE,8,EGL10.EGL_GREEN_SIZE,8,EGL10.EGL_BLUE_SIZE,8,EGL10.EGL_NONE),configs,1,count) && count[0]>0) configs[0] ?: config else config
            val context = egl.eglCreateContext(display, rootConfig, EGL10.EGL_NO_CONTEXT, intArrayOf(0x3098,3,EGL10.EGL_NONE))
            val surface = if (context != EGL10.EGL_NO_CONTEXT) egl.eglCreatePbufferSurface(display,rootConfig,intArrayOf(EGL10.EGL_WIDTH,1,EGL10.EGL_HEIGHT,1,EGL10.EGL_NONE)) else EGL10.EGL_NO_SURFACE
            if (context != EGL10.EGL_NO_CONTEXT && surface != EGL10.EGL_NO_SURFACE) {
                val previousContext = egl.eglGetCurrentContext()
                val previousDisplay = egl.eglGetCurrentDisplay().let { if(it==EGL10.EGL_NO_DISPLAY) display else it }
                val previousDraw = egl.eglGetCurrentSurface(EGL10.EGL_DRAW)
                val previousRead = egl.eglGetCurrentSurface(EGL10.EGL_READ)
                if (egl.eglMakeCurrent(display,surface,surface,context)) {
                    val captured = try { NativeBridge.nativeCapturePresentationRoot(id) }
                        finally { egl.eglMakeCurrent(previousDisplay,previousDraw,previousRead,previousContext) }
                    if (captured) { root=context;rootSurface=surface;rootDisplay=display }
                }
            }
            if (root == EGL10.EGL_NO_CONTEXT) {
                if(surface != EGL10.EGL_NO_SURFACE) egl.eglDestroySurface(display,surface)
                if(context != EGL10.EGL_NO_CONTEXT) egl.eglDestroyContext(display,context)
                Log.w("PocketHLE","PRESENTATION=CPU_UPLOAD reason=android_shared_root_unavailable")
            }
        }
        val context = if(root != EGL10.EGL_NO_CONTEXT) egl.eglCreateContext(display,config,root,intArrayOf(0x3098,3,EGL10.EGL_NONE)) else EGL10.EGL_NO_CONTEXT
        sharedUi = context != EGL10.EGL_NO_CONTEXT
        return if(sharedUi) context else egl.eglCreateContext(display,config,EGL10.EGL_NO_CONTEXT,intArrayOf(0x3098,3,EGL10.EGL_NONE))
    }
    @Synchronized fun isShared(): Boolean = sharedUi && !closing

    @Synchronized override fun destroyContext(egl: EGL10, display: EGLDisplay, context: EGLContext) {
        NativeBridge.nativeDetachPresentation(id)
        // Shared texture/program names survive child-context destruction. Delete
        // only those shared objects through the retained root; FBOs/VAOs/queries
        // belong to the old UI context and are destroyed with that context.
        if(sharedUi && root != EGL10.EGL_NO_CONTEXT && egl.eglMakeCurrent(rootDisplay,rootSurface,rootSurface,root)) {
            try { renderer.releaseSharedObjects() }
            finally { egl.eglMakeCurrent(rootDisplay,EGL10.EGL_NO_SURFACE,EGL10.EGL_NO_SURFACE,EGL10.EGL_NO_CONTEXT) }
        }
        egl.eglDestroyContext(display,context)
        sharedUi=false
    }
    /** May run off the UI thread: root is never left current outside factory operations. */
    @Synchronized fun closeAfterJoin() {
        if(closing) return
        closing=true
        NativeBridge.nativeClosePresentation(id)
        val egl=api
        if(egl != null && root != EGL10.EGL_NO_CONTEXT) {
            egl.eglDestroySurface(rootDisplay,rootSurface)
            egl.eglDestroyContext(rootDisplay,root)
        }
        root=EGL10.EGL_NO_CONTEXT;rootSurface=EGL10.EGL_NO_SURFACE
    }
}
