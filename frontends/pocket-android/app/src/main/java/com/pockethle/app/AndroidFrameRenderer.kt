package com.pockethle.app

import android.content.Context
import android.graphics.Bitmap
import android.opengl.GLES30.*
import android.opengl.GLSurfaceView
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.widget.Toast
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10

/** Desktop shaders, including reference three-pass SMAA; filtering never alters guest geometry. */
internal class AndroidFrameRenderer(context: Context, private val filter: String, private val scale: Int) : GLSurfaceView.Renderer {
    private val app = context.applicationContext
    @Volatile private var pending: GameActivity.FrameSnapshot? = null
    @Volatile private var turn = 0
    @Volatile private var capture = false
    private var uploaded: GameActivity.FrameSnapshot? = null
    private var texture = 0
    private var program = 0
    private var vao = 0
    private var textureW = 0
    private var textureH = 0
    private var viewW = 1
    private var viewH = 1
    private var smaa: Smaa? = null
    private var filtered = 0
    private var failed = false
    fun submit(frame: GameActivity.FrameSnapshot) { pending = frame }
    fun setRotationDegrees(degrees: Int) { turn = ((degrees % 360)+360)%360 }
    fun requestScreenshot() { capture = true }

    override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
        failed = false; uploaded = null; textureW = 0; textureH = 0
        // Context recreation destroys old GL names; do not try to delete them in the new context.
        smaa = null
        try {
            program = compileProgram(asset("reconstruction.vert"), asset("reconstruction.frag"))
            val names = IntArray(1); glGenVertexArrays(1,names,0); vao=names[0]; glBindVertexArray(vao)
            texture = createTexture(1,1,GL_RGBA8,GL_RGBA,null)
            if (filter == "smaa" || filter == "smaa_soft") smaa = Smaa()
        } catch (e: Exception) { fail(e) }
    }
    override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) { viewW=width;viewH=height }
    override fun onDrawFrame(gl: GL10?) {
        glBindFramebuffer(GL_FRAMEBUFFER,0);glViewport(0,0,viewW,viewH)
        glDisable(GL_SCISSOR_TEST);glDisable(GL_BLEND);glDisable(GL_DEPTH_TEST)
        glClearColor(0f,0f,0f,1f);glClear(GL_COLOR_BUFFER_BIT)
        if (failed) return
        val frame = pending ?: return
        try {
            glBindVertexArray(vao)
            if (uploaded !== frame) {
                var bytes = frame.rgba.copyOfRange(frame.rgbaOffset, frame.rgbaOffset+frame.width*frame.height*4)
                textureW=frame.width;textureH=frame.height
                if (filter == "xbrz") {
                    bytes = NativeBridge.upscaleXbrz(bytes,frame.width,frame.height) ?: error("xBRZ failed")
                    textureW *= 3; textureH *= 3
                }
                glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_2D,texture)
                glPixelStorei(GL_UNPACK_ALIGNMENT,1)
                glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,textureW,textureH,0,GL_RGBA,GL_UNSIGNED_BYTE,direct(bytes))
                filtered = smaa?.process(texture,textureW,textureH,filter == "smaa_soft") ?: texture
                uploaded=frame
            }
            glBindFramebuffer(GL_FRAMEBUFFER,0)
            val quarter = turn == 90 || turn == 270
            val w = if (quarter) frame.height else frame.width
            val h = if (quarter) frame.width else frame.height
            val rect = displayRect(viewW,viewH,w,h,scale)
            val drawnW=rect.width;val drawnH=rect.height
            val x=rect.left;val y=viewH-rect.top-drawnH
            glViewport(x,y,drawnW,drawnH);glUseProgram(program)
            glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_2D,filtered)
            glUniform1i(glGetUniformLocation(program,"u_frame"),0)
            glUniform2f(glGetUniformLocation(program,"u_size"),textureW.toFloat(),textureH.toFloat())
            val mode = when(filter) { "nearest" -> 0; "bilinear","xbrz","smaa_soft" -> 1; "bicubic" -> 2; "lanczos" -> 4; else -> 3 }
            glUniform1i(glGetUniformLocation(program,"u_filter"),mode)
            val uv = when(turn) { 90 -> floatArrayOf(0f,1f,0f,-1f,1f,0f);180 -> floatArrayOf(1f,1f,-1f,0f,0f,-1f);270 -> floatArrayOf(1f,0f,0f,1f,-1f,0f);else -> floatArrayOf(0f,0f,1f,0f,0f,1f) }
            listOf("u_origin","u_dx","u_dy").forEachIndexed { i,name -> glUniform2f(glGetUniformLocation(program,name),uv[i*2],uv[i*2+1]) }
            glDrawArrays(GL_TRIANGLES,0,3)
            if (capture) { capture=false; saveScreenshot(x,y,drawnW,drawnH) }
        } catch (e: Exception) { fail(e) }
    }
    private fun fail(e: Exception) {
        failed=true;Log.e("PocketHLE","Android presentation failed ($filter)",e)
        Handler(Looper.getMainLooper()).post { Toast.makeText(app,"Erreur du rendu $filter : ${e.message}",Toast.LENGTH_LONG).show() }
    }
    private fun saveScreenshot(x: Int,y: Int,w: Int,h: Int) {
        val pixels=ByteBuffer.allocateDirect(w*h*4).order(ByteOrder.nativeOrder())
        glReadPixels(x,y,w,h,GL_RGBA,GL_UNSIGNED_BYTE,pixels)
        check(glGetError()==GL_NO_ERROR) { "Screenshot readback failed" }
        val colors=IntArray(w*h)
        for (row in 0 until h) for (col in 0 until w) {
            val offset=((h-1-row)*w+col)*4
            colors[row*w+col]=((pixels.get(offset+3).toInt() and 255) shl 24) or ((pixels.get(offset).toInt() and 255) shl 16) or ((pixels.get(offset+1).toInt() and 255) shl 8) or (pixels.get(offset+2).toInt() and 255)
        }
        Thread {
            runCatching {
                val directory=File(LibraryPaths.root(app),"screenshots");directory.mkdirs()
                val file=File(directory,"game-${System.currentTimeMillis()}.png")
                val bitmap=Bitmap.createBitmap(colors,w,h,Bitmap.Config.ARGB_8888)
                try { file.outputStream().use { check(bitmap.compress(Bitmap.CompressFormat.PNG,100,it)) } } finally { bitmap.recycle() }
                Handler(Looper.getMainLooper()).post { Toast.makeText(app,"Capture : ${file.name}",Toast.LENGTH_SHORT).show() }
            }.onFailure { Log.e("PocketHLE","Screenshot save failed",it) }
        }.start()
    }
    private fun asset(name: String) = app.assets.open("shaders/$name").bufferedReader().use { it.readText() }
    private fun direct(bytes: ByteArray) = ByteBuffer.allocateDirect(bytes.size).apply { put(bytes);position(0) }
    private fun compileProgram(vertex: String, fragment: String): Int {
        val prefix="#version 300 es\nprecision highp float;\nprecision highp int;\n"
        fun shader(type: Int, source: String): Int {
            val id=glCreateShader(type);glShaderSource(id,prefix+source);glCompileShader(id)
            val status=IntArray(1);glGetShaderiv(id,GL_COMPILE_STATUS,status,0)
            if (status[0]==0) { val message=glGetShaderInfoLog(id);glDeleteShader(id);error(message) }
            return id
        }
        val vs=shader(GL_VERTEX_SHADER,vertex)
        val fs=try { shader(GL_FRAGMENT_SHADER,fragment) } catch (e: Exception) { glDeleteShader(vs);throw e }
        val id=glCreateProgram();glAttachShader(id,vs);glAttachShader(id,fs);glLinkProgram(id)
        glDeleteShader(vs);glDeleteShader(fs)
        val status=IntArray(1);glGetProgramiv(id,GL_LINK_STATUS,status,0)
        if(status[0]==0) { val message=glGetProgramInfoLog(id);glDeleteProgram(id);error(message) }
        return id
    }
    private fun createTexture(w: Int,h: Int,internal: Int,format: Int,bytes: ByteArray?): Int {
        val names=IntArray(1);glGenTextures(1,names,0);glBindTexture(GL_TEXTURE_2D,names[0])
        glPixelStorei(GL_UNPACK_ALIGNMENT,1)
        glTexImage2D(GL_TEXTURE_2D,0,internal,w,h,0,format,GL_UNSIGNED_BYTE,bytes?.let { direct(it) })
        for (param in listOf(GL_TEXTURE_MIN_FILTER,GL_TEXTURE_MAG_FILTER)) glTexParameteri(GL_TEXTURE_2D,param,GL_LINEAR)
        for (param in listOf(GL_TEXTURE_WRAP_S,GL_TEXTURE_WRAP_T)) glTexParameteri(GL_TEXTURE_2D,param,GL_CLAMP_TO_EDGE)
        return names[0]
    }
    private inner class Smaa {
        private val programs = listOf("edges.frag","weights.frag","blend.frag").map {
            val header="uniform vec4 u_metrics;\nuniform float u_threshold;\n#define SMAA_RT_METRICS u_metrics\n#define SMAA_GLSL_3 1\n#define SMAA_MAX_SEARCH_STEPS 16\n#define SMAA_MAX_SEARCH_STEPS_DIAG 8\n#define SMAA_CORNER_ROUNDING 25\n#define SMAA_THRESHOLD u_threshold\n"
            compileProgram(asset("reconstruction.vert"),header+asset("smaa/SMAA.glsl")+"\n"+asset("smaa/$it"))
        }
        private val area=createTexture(160,560,GL_RG8,GL_RG,app.assets.open("shaders/smaa/AreaTex.bin").use { it.readBytes() })
        private val search=createTexture(64,16,GL_R8,GL_RED,app.assets.open("shaders/smaa/SearchTex.bin").use { it.readBytes() })
        private val targets=IntArray(3)
        private val fbos=IntArray(3)
        private var width=0
        private var height=0
        fun process(input: Int,w: Int,h: Int,soft: Boolean): Int {
            if(width != w || height != h) {
                if(width != 0) { glDeleteTextures(3,targets,0);glDeleteFramebuffers(3,fbos,0) }
                glGenFramebuffers(3,fbos,0)
                for(i in 0..2) {
                    targets[i]=createTexture(w,h,GL_RGBA8,GL_RGBA,null)
                    glBindFramebuffer(GL_FRAMEBUFFER,fbos[i]);glFramebufferTexture2D(GL_FRAMEBUFFER,GL_COLOR_ATTACHMENT0,GL_TEXTURE_2D,targets[i],0)
                    check(glCheckFramebufferStatus(GL_FRAMEBUFFER)==GL_FRAMEBUFFER_COMPLETE) { "SMAA framebuffer incomplete" }
                }
                width=w;height=h
            }
            for(i in 0..2) {
                glBindFramebuffer(GL_FRAMEBUFFER,fbos[i]);glViewport(0,0,w,h);glClearColor(0f,0f,0f,0f);glClear(GL_COLOR_BUFFER_BIT)
                val p=programs[i];glUseProgram(p)
                glUniform4f(glGetUniformLocation(p,"u_metrics"),1f/w,1f/h,w.toFloat(),h.toFloat())
                glUniform1f(glGetUniformLocation(p,"u_threshold"),if(soft) .05f else .1f)
                val textures=when(i) { 0 -> listOf("u_color" to input);1 -> listOf("u_edges" to targets[0],"u_area" to area,"u_search" to search);else -> listOf("u_color" to input,"u_blend" to targets[1]) }
                textures.forEachIndexed { unit,(name,t) -> glActiveTexture(GL_TEXTURE0+unit);glBindTexture(GL_TEXTURE_2D,t);glUniform1i(glGetUniformLocation(p,name),unit) }
                glDrawArrays(GL_TRIANGLES,0,3)
            }
            return targets[2]
        }
    }
}
