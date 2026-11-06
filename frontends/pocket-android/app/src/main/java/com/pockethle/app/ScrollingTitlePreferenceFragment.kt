package com.pockethle.app

import android.text.TextUtils
import android.widget.TextView
import androidx.preference.PreferenceFragmentCompat
import androidx.preference.PreferenceGroupAdapter
import androidx.preference.PreferenceScreen
import androidx.preference.PreferenceViewHolder
import androidx.recyclerview.widget.RecyclerView

/** Keep preference rows tappable while Android scrolls titles that exceed their width. */
abstract class ScrollingTitlePreferenceFragment : PreferenceFragmentCompat() {
    override fun onCreateAdapter(preferenceScreen: PreferenceScreen): RecyclerView.Adapter<*> =
        object : PreferenceGroupAdapter(preferenceScreen) {
            override fun onBindViewHolder(holder: PreferenceViewHolder, position: Int) {
                super.onBindViewHolder(holder, position)
                (holder.findViewById(android.R.id.title) as? TextView)?.apply {
                    setSingleLine(true)
                    ellipsize = TextUtils.TruncateAt.MARQUEE
                    marqueeRepeatLimit = -1
                    isSelected = true
                }
            }
        }
}
