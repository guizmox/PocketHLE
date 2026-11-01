# Gizmondo desktop controls

Apply after the preceding diagnostics/resolution, resolution-policy/Steam Deck
and recording-label patches. This patch retains those changes in shared files.
The source reference is guizmox/PocketHLE b0422921e1d490932329b059b924a76487e76a94.
Only Windows/Linux/Steam Deck presentation is changed; Android layout is unchanged.

The photographed console skin is replaced by a dark PocketPC-style control panel.
The five function buttons stay above the game: Home, Volume, Brightness, Geofence
and Power. Geofence is drawn as a vector GPS antenna. L/R are taller downward;
the four media action buttons are circles in the Stop/Rewind/Forward/Play diamond.
The existing cross D-pad stays at the left. All guest controls use the user's
existing bindings and illuminate for held keyboard/controller/pointer input.
Guest key codes are unchanged (including Power/Piano 5 = F11).

Fullscreen Gizmondo now displays this same panel, including on Steam Deck, with
Exit/Full controls. PocketPC fullscreen behavior is unchanged. Image geometry uses
rendered pixels for zoom and logical pixels for guest input. Controls never cover
the game. Small viewports fit the whole image; Steam Deck fullscreen preserves
its aspect-fit policy. Window sizing reserves the same controls/header as drawing
and guards physical-pixel rounding at fractional DPI against lost integer zoom.
Round action buttons also have round pointer hit tests.

Recording labels use the Gizmondo profile. Brightness and Geofence are written in
full; the existing video marker expands horizontally for longer function names.
The PocketPC label profile and short circular video markers are retained.

removed-files.txt contains the unused desktop skin asset, now unreferenced.
patch-files.txt contains relative Windows payload paths. No game file is modified.

Targeted checks: eight geometry tests passed across viewport sizes, rendered
resolutions, DPI values and window/fullscreen scale policies, including control
separation and round hit tests. Three recording tests passed using the actual
label/state/overlay source: both profiles, profile transitions, full long names,
and pixel-identical short markers. Changed Rust files pass rustfmt. No complete
Windows/Linux/Android build, GitHub CI run or physical Steam Deck run was performed.
