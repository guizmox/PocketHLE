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
            caption.isSelected = true
            val artwork = itemView.findViewById<android.widget.ImageView>(R.id.game_artwork)
            val label = itemView.findViewById<View>(R.id.game_label)
            val gizmondo = NativeBridge.isGizmondoGame(libraryRoot, entry.id)
            val bitmap = entry.icon?.takeIf { !gizmondo || it == "cartridge.png" }?.let {
                BitmapFactory.decodeFile(File(libraryRoot,"games/${entry.id}/$it").absolutePath)
            }
            val showArtwork = !gizmondo || bitmap != null
            itemView.findViewById<View>(R.id.game_card_fallback).visibility = if (showArtwork) View.GONE else View.VISIBLE
            if (gizmondo) artwork.setImageBitmap(bitmap) else artwork.setImageDrawable(PocketPcArtworkDrawable(bitmap))
            artwork.visibility = if (showArtwork) View.VISIBLE else View.GONE
            label.visibility = if (showArtwork) View.GONE else View.VISIBLE
            itemView.contentDescription = entry.displayName
            publisherLabel.text = "${if (gizmondo) "GIZMONDO" else "POCKET PC / WINCE"} • ${entry.screenWidth}×${entry.screenHeight}"
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
            popup.menu.add(0, 5, 5, "Game ID")
            popup.setOnMenuItemClickListener { item ->
                when (item.itemId) {
                    2 -> onRun(entry)
                    3 -> onRename(entry)
                    4 -> onInformation(entry)
                    0 -> onSettings(entry)
                    1 -> onRemove(entry)
                    5 -> {
                        val context = itemView.context
                        val clipboard = context.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
                        clipboard.setPrimaryClip(android.content.ClipData.newPlainText("Game ID", entry.id))
                        androidx.appcompat.app.AlertDialog.Builder(context)
                            .setTitle("Game ID")
                            .setMessage("${entry.id}\n\nCopied to clipboard.")
                            .setPositiveButton(android.R.string.ok, null).show()
                    }
                }
                true
            }
            popup.show()
        }
    }
}
