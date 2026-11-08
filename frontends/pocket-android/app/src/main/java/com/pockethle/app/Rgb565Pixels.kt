package com.pockethle.app

/** CPU preprocessing fallback for xBRZ; ordinary packed uploads do not call it. */
internal fun expandRgb565(pixels: ByteArray, offset: Int, width: Int, height: Int, backlight: Int): ByteArray {
    val count=width.toLong()*height
    require(width>0 && height>0 && offset>=0 && count<=Int.MAX_VALUE/4 && count*2<=pixels.size.toLong()-offset)
    val level=if(backlight in 1..31) backlight else 11
    fun light(c:Int)=if(level<=11) c-c*(11-level)/50 else c+(255-c)*(level-11)/240
    val out=ByteArray(count.toInt()*4)
    for(i in 0 until count.toInt()) {
        val p=(pixels[offset+i*2].toInt() and 255) or ((pixels[offset+i*2+1].toInt() and 255) shl 8)
        val r=(p ushr 11) and 31; val g=(p ushr 5) and 63;val b=p and 31
        out[i*4]=light((r shl 3) or (r ushr 2)).toByte()
        out[i*4+1]=light((g shl 2) or (g ushr 4)).toByte()
        out[i*4+2]=light((b shl 3) or (b ushr 2)).toByte();out[i*4+3]=(-1).toByte()
    }
    return out
}
