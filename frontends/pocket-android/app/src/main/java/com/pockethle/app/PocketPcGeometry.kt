package com.pockethle.app

/** Host pixels only. The guest display and its stylus coordinates never change here. */
internal data class ControlRect(val x: Int, val y: Int, val w: Int, val h: Int)
internal data class PocketPcGeometry(
    val screen: ControlRect,
    val actions: List<ControlRect>,
    val auxiliary: List<ControlRect>,
    val dpad: ControlRect,
    val stick: ControlRect,
) {
    companion object {
        // Action order: A, P2, Action, P1, P4, C, P3, B.
        // Auxiliary order: Tab, Esc, Turbo, Exit, Rotate.
        fun calculate(width: Int, height: Int, density: Float): PocketPcGeometry {
            val w=width.coerceAtLeast(1); val h=height.coerceAtLeast(1)
            fun dp(n: Float)=(n*density).toInt().coerceAtLeast(1)
            val gap=dp(6f); val safety=dp(18f)
            if(w>h) {
                val side=minOf(dp(140f),w/5,h/2).coerceAtLeast(1)
                val pad=minOf(side,h/3).coerceAtLeast(1)
                val padY=h-gap-pad
                val actionBottom=(padY-safety).coerceAtLeast(gap+2)
                val cellW=((side-gap)/2).coerceAtLeast(1)
                val cellH=((actionBottom-2*gap)/2).coerceAtLeast(1)
                fun bank(x: Int)=listOf(ControlRect(x,gap,cellW,cellH),ControlRect(x+cellW+gap,gap,cellW,cellH),
                    ControlRect(x,gap+cellH+gap,cellW,cellH),ControlRect(x+cellW+gap,gap+cellH+gap,cellW,cellH))
                val left=side+2*gap; val room=(w-2*left).coerceAtLeast(1)
                val toolbarH=minOf(dp(32f),h/9).coerceAtLeast(1)
                // Wider guest keys, with a distinct gap before the orange host actions.
                val bw=minOf(dp(64f),((room-5*gap)/5).coerceAtLeast(1))
                val aux=(0..2).map { ControlRect(left+it*(bw+gap),gap,bw,toolbarH) } +
                    listOf(ControlRect(w-left-2*bw-gap,gap,bw,toolbarH),ControlRect(w-left-bw,gap,bw,toolbarH))
                return PocketPcGeometry(ControlRect(left,toolbarH+2*gap,room,(h-toolbarH-3*gap).coerceAtLeast(1)),
                    bank(gap)+bank(w-side-gap),aux,ControlRect(gap,padY,pad,pad),ControlRect(w-gap-pad,padY,pad,pad))
            }
            val pad=minOf(dp(112f),(w-6*gap)/3).coerceAtLeast(1)
            val deckH=maxOf(pad,dp(108f))
            val deckY=h-gap-deckH
            val rowH=minOf(dp(44f),h/14).coerceAtLeast(1)
            val actionY=deckY-safety-2*rowH-gap
            val cellW=((w-5*gap)/4).coerceAtLeast(1)
            val rowOrder=listOf(0,1,4,5,2,3,6,7)
            val actions=MutableList(8) { ControlRect(0,0,1,1) }
            rowOrder.forEachIndexed { i,index -> actions[index]=ControlRect(gap+(i%4)*(cellW+gap),actionY+(i/4)*(rowH+gap),cellW,rowH) }
            // Portrait tools stay above the display; separate host actions
            // from the three guest keys by at least three ordinary gaps.
            val toolbarH=rowH
            val bw=minOf(dp(64f),((w-8*gap)/5).coerceAtLeast(1))
            val aux=(0..2).map { ControlRect(gap+it*(bw+gap),gap,bw,toolbarH) } +
                listOf(ControlRect(w-2*gap-2*bw,gap,bw,toolbarH),ControlRect(w-gap-bw,gap,bw,toolbarH))
            val screenY=toolbarH+2*gap
            return PocketPcGeometry(ControlRect(0,screenY,w,(actionY-gap-screenY).coerceAtLeast(1)),actions,aux,
                ControlRect(gap,h-gap-pad,pad,pad),ControlRect(w-gap-pad,h-gap-pad,pad,pad))
        }
    }
}
