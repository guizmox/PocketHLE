package com.pockethle.app

import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.graphics.BitmapFactory
import java.io.File
import android.widget.ImageButton
import android.widget.PopupMenu
import android.widget.TextView
import androidx.recyclerview.widget.RecyclerView

/**
 * RecyclerView adapter for the library screen — launcher-style grid
 * of game tiles (cover art + title). Tapping a tile runs the game;
 * Settings/Remove live behind the tile's overflow (⋮) button so the
 * grid stays visually clean.
 */
class GameAdapter(
    private val onRun: (GameEntry) -> Unit,
    private val onSettings: (GameEntry) -> Unit,
    private val onRemove: (GameEntry) -> Unit,
    private val onRename: (GameEntry) -> Unit,
    private val onInformation: (GameEntry) -> Unit,
    private val libraryRoot: String,
) : RecyclerView.Adapter<GameAdapter.ViewHolder>() {

    private var items: List<GameEntry> = emptyList()

    fun submit(newItems: List<GameEntry>) {
        items = newItems
        notifyDataSetChanged()
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_game_grid, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        holder.bind(items[position])
    }

    override fun getItemCount(): Int = items.size

    inner class ViewHolder(view: View) : RecyclerView.ViewHolder(view) {
        private val title: TextView = view.findViewById(R.id.game_title)
        private val caption: TextView = view.findViewById(R.id.game_caption)
        private val serial: TextView = view.findViewById(R.id.game_serial)
        private val publisherLabel: TextView = view.findViewById(R.id.game_publisher)
        private val moreBtn: ImageButton = view.findViewById(R.id.btn_more)

        fun bind(entry: GameEntry) {
            title.text = entry.displayName
            caption.text = entry.displayName
            val artwork = itemView.findViewById<android.widget.ImageView>(R.id.game_artwork)
            val label = itemView.findViewById<View>(R.id.game_label)
            val bitmap = if (entry.icon == "cartridge.png") BitmapFactory.decodeFile(File(libraryRoot,"games/${entry.id}/cartridge.png").absolutePath) else null
            itemView.findViewById<View>(R.id.game_card_fallback).visibility = if (bitmap != null) View.GONE else View.VISIBLE
            artwork.setImageBitmap(bitmap)
            artwork.visibility = if (bitmap != null) View.VISIBLE else View.GONE
            label.visibility = if (bitmap != null) View.GONE else View.VISIBLE
            itemView.contentDescription = entry.displayName
            publisherLabel.text = if (NativeBridge.isGizmondoGame(libraryRoot, entry.id)) "GIZMONDO • 320×240" else "POCKET PC / WINCE"
            serial.text = entry.titleId
            serial.visibility = if (entry.titleId != null) View.VISIBLE else View.GONE
            itemView.setOnClickListener { onRun(entry) }
            moreBtn.setOnClickListener { anchor -> showOverflowMenu(anchor, entry) }
        }

        private fun showOverflowMenu(anchor: View, entry: GameEntry) {
            val popup = PopupMenu(anchor.context, anchor)
            popup.menu.add(0, 2, 0, R.string.action_play)
            popup.menu.add(0, 4, 1, R.string.action_information)
            popup.menu.add(0, 3, 2, "Rename")
            popup.menu.add(0, 0, 3, R.string.action_settings)
            popup.menu.add(0, 1, 4, R.string.action_remove)
            popup.setOnMenuItemClickListener { item ->
                when (item.itemId) {
                    2 -> onRun(entry)
                    3 -> onRename(entry)
                    4 -> onInformation(entry)
                    0 -> onSettings(entry)
                    1 -> onRemove(entry)
                }
                true
            }
            popup.show()
        }
    }
}
