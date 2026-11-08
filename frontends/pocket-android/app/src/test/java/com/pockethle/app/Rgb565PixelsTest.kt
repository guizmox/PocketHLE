package com.pockethle.app
import org.junit.Assert.*
import org.junit.Test

class Rgb565PixelsTest {
    @Test fun fallbackUsesBitReplicationAndIntegerBacklightBeforeXbrz() {
        val raw=byteArrayOf(9,9,0,0,(-1).toByte(),(-1).toByte(),3,0)
        for(level in listOf(1,11,21,31,0,99)) {
            val pixels=expandRgb565(raw,2,3,1,level)
            val l=if(level in 1..31) level else 11
            fun light(c:Int)=if(l<=11) c-c*(11-l)/50 else c+(255-c)*(l-11)/240
            assertArrayEquals(byteArrayOf(light(0).toByte(),light(0).toByte(),light(0).toByte(),(-1).toByte()),pixels.copyOfRange(0,4))
            assertArrayEquals(byteArrayOf(light(255).toByte(),light(255).toByte(),light(255).toByte(),(-1).toByte()),pixels.copyOfRange(4,8))
            assertEquals(light(24),pixels[10].toInt() and 255)
        }
    }
    @Test(expected=IllegalArgumentException::class) fun rejectsTruncatedSource() {
        expandRgb565(ByteArray(5),0,3,1,11)
    }
}
