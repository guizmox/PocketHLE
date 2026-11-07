# Flat input skins

Project-authored, unbranded flat artwork. No generated texture or third-party
controller logos are used. SVG files are editable originals; PNG files are their
2x raster exports, loaded by the existing desktop image dependency.

`keyboard-layout.json` is the keyboard's geometry, in a 1000 × 340 canvas.
Its rectangles match the SVG keycaps exactly. An `input` of null marks a decorative
modifier keycap; other input names are canonicalized through `egui::Key` at load.
Change the SVG, PNG and hitbox geometry together when changing the layout.

The controller canvas is 760 × 500. Its hotspot coordinates are in
`controller-layout.json`, loaded by both frontends. Compact live assignments are
drawn on the device, with selection rows around/below it. Extra/Misc1 remains in the list because generic controllers need not
have a physical extra button. Both profiles use the same artwork.

On-device captions use short forms (Act, S1/S2, P1–P4, PenU/D/L/R) to fit real
control spacing. Full command names remain in the selection inspector and rows.

The 26 controller entries are ordered as 13 left-rail controls, then 13 right-rail
controls. Both editors use this order for the corresponding selection rows.
