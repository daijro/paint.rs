# paint.rs icon style

This guide covers the "paint.rs" icon pack: flat, filled, coloured SVG icons in the spirit of
Microsoft Fluent's filled and colour icons. The pack has one file per Paint.NET icon name. It is
original artwork and is the default pack bundled with paint.rs.

The seed icons in `assets/icons-svg/` are the reference implementation of this guide. Before you
draw, open them next to this file. When this text and a seed disagree, the seed wins; report the
mismatch.

## 1. Originality (read first)

- Look at the Paint.NET icon only to learn what it depicts: its concept, its metaphor ("folder
  with an arrow") and its colour cue ("blue tile", "red ×"). Then close it and draw your own
  geometry on the grid below.
- Never trace, re-draw pixel by pixel or copy Paint.NET's exact composition. Never copy a PNG into
  the repo.
- **Keep Paint.NET's core metaphor.** Each remake keeps the main object the original shows, redrawn
  in this style: a wrench for layer properties, a rotate arrow, a moving ball, a sun / a bulb for
  global / local flood, an envelope for feedback, an open book for tutorials, a goblet for frosted
  glass. Don't swap in a different metaphor, even a more literal one. Colours follow §5 and
  modifiers may become §7 badges, but the hero object stays. Only §10 (third-party marks) overrides
  this.
- Draw no third-party logos or wordmarks. That rules out PayPal, Windows, the Paint.NET logo and
  any text. Draw a generic stand-in instead (see §10).
- You may consult Microsoft Fluent UI System Icons (MIT) for reference: `npm run fluent-ref` unpacks
  it into the git-ignored `fluent-ref/`. Prefer original geometry. If an icon's geometry is
  derived from a Fluent icon, say so in your batch report. The controller then appends the name
  to `derived-from-fluent.txt`, sets `"fluentDerived": true` in
  `assets/icons-svg/manifest.json`, and restores the MIT notice (see that file's header). No icon
  is derived today, so the pack ships no Fluent notice. Batch agents do not edit those shared
  files themselves, to avoid merge conflicts.

## 2. Files

- Path: `assets/icons-svg/<name>.svg`, with `<name>` exactly as in `icons.txt`. There is one file
  per name.
- Root element, always in this form, attributes in this order:
  `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16">`. Width,
  height and viewBox equal the icon's **Paint.NET 96-DPI pixel size**; see §9 for the icons
  that aren't 16×16.
- Allowed elements: `path rect circle ellipse line polyline polygon g defs use mask`.
- Allowed attributes: geometry attributes, `fill`, `stroke`, `stroke-width`, `stroke-linecap`,
  `stroke-linejoin`, `stroke-dasharray`, `stroke-dashoffset`, `fill-rule`, `transform`, `mask`, `id`
  and `href` (internal `#id` only).
- Forbidden: `<text>`, `<image>`, `<style>`, `style=""`, `<script>`, event handlers, filters,
  radial gradients, patterns, `opacity`/`fill-opacity`/`stroke-opacity`, `xlink:` and any external
  reference.
- **Exceptions (user, 2026-10-19).** Gradients are allowed only where a gradient *is* the subject:
  - motion and blur trails: a `<linearGradient>` fading ONE palette colour from `stop-opacity="0"` to opaque
    (`stop-opacity` is allowed only on gradient stops), e.g. `motion_blur_effect`;
  - gradient icons (the gradient tool, `enum_gradient_type_*`, `gradient_is_alpha_only_*`, HSV gradients):
    `<linearGradient>`/`<radialGradient>` with `gradientUnits="userSpaceOnUse"` and optional `spreadMethod`,
    stops from the palette, plus `<clipPath>` where needed. SVG has no conic gradient, so a conical
    gradient is drawn as ~20 overlapping palette-interpolated wedges over a mid-tone base.
  The user asked for "actual gradients" on the gradient icons; flat stripes standing in for a gradient are wrong.
- Colours: only the lowercase hex values from §5, `none`, and `#fff`/`#000` inside badge masks.
  Never use `currentColor`.
- Keep the file small. Write it on one line, with no comments, metadata or editor cruft, and at
  most 2 decimals per number. The target is under 1.5 KB; the seeds weigh 150–500 B.
- `.dark` names (11 of them, e.g. `move_tool_icon.dark`) normally get **no file**. See §6.

## 3. Grid and keylines (16 × 16)

- **Live area** spans 1–15 on both axes, leaving 1 px of padding. Only badges (§7) may touch the
  16 edge.
- **Keylines.** Fit the main shape to one of these:
  | keyline   | box                         | e.g.                          |
  |-----------|-----------------------------|-------------------------------|
  | square    | 2–14 × 2–14 (12×12)          | floppy, layer tile (10×10 at 2) |
  | circle    | Ø14 centred on 8,8 (1–15)    | cancel disc, gear             |
  | portrait  | 3–13 × 1–15 (10×14)          | document                      |
  | landscape | 1–15 × 3–13 (14×10)          | picture, marquee              |
- **Corner radii** are outer radii, so a stroked rect at `.5` uses `rx` = outer − 0.5:
  - shapes 8 px and larger: 1.5 (filled), or `rx="1"` on a 1-px stroked rect;
  - shapes of 4–7 px: 1;
  - smaller shapes: 0–0.5;
  - pills and bars: half the height.
- **Minimum gap** between two separate shapes is 1 px, and no detail is smaller than 1 px. Two
  shapes of the same colour need a gap, or a different tone where they meet.
- On **24 / 32 / 64 px** renders the icon scales as vector. Don't add detail that only works when
  large: the 16-px read decides.
- **No stray detail.** Drop any line or highlight bar that doesn't read at 16 px: Tint or Paper
  strokes on Paper (a floppy's label line), a line across a cloud, a glint that crosses a glyph (the
  zoom +/− lenses have none). A glint is allowed only on otherwise empty glass (zoom tool, blur drop).
  For depth, use a Shade face (§5), not a Tint line (`sharpen_effect`).

## 4. Construction and pixel alignment (filled style)

- **Fill first.** Every object with area is a filled shape: tiles, bodies, discs, drops, folders.
- **Light containers** get a 1-px border. These are objects whose fill is Paper or a Tint: paper,
  glass, picture frames, selections. Draw the border as `stroke` (width 1) on `.5` coordinates,
  in the matching Base colour (Slate for Paper).
  Example: `<rect x="5.5" y="4.5" width="8" height="10" rx="1" fill="#d7e6f6" stroke="#5b95d6"/>`.
- **Linework** covers curves, guide lines, arrows' shafts, dashes and text lines:
  - axis-aligned lines are 1 px on `.5` or 2 px on whole coordinates, so they render crisp;
  - diagonal and curved lines are 1.5–2.5 px with `stroke-linecap="round"`;
  - nothing is thinner than 1 px.
- **Axis-aligned edges** of filled shapes sit on whole pixels. Stroked edges sit on `.5`.
- **Rounded joins:** to soften a polygon (arrowheads, pointers, triangles), add a same-colour
  stroke of width 1–1.5 with `stroke-linejoin="round"` and shrink the shape by half the width.
  See `warning_icon` and `menu_edit_undo_icon`.
- **Dashed marquee** (the selection cue): `stroke-dasharray="1 2"` plus
  `stroke-linecap="square"`. That renders as crisp 2-on/1-off dashes. Each side's centre-line
  length must be a multiple of 3 (6, 9 or 12) so every corner gets a dash.
- Light comes from the top-left (§5, Shading).

## 5. Colour

### Palette

Each hue has three tones. **Base** is the main fill and any colour on the silhouette. **Shade**
is for back faces, depth and small details. **Tint** is for glass, sky, light interiors and
selections.

| hue     | Base      | Shade     | Tint      | role                                                                                                   |
|---------|-----------|-----------|-----------|--------------------------------------------------------------------------------------------------------|
| Blue    | `#5b95d6` | `#3c74b4` | `#d7e6f6` | primary: selection, layers, canvas/picture, arrows and navigation, zoom lens, info, links, "result" objects |
| Slate   | `#737a85` | `#4b525c` | `#e3e7ec` | neutral objects: tool metal, gear, trash, printer, scanner, door, text marks, diagram lines, paper borders |
| Green   | `#24a05a` | `#14773f` | `#cbefd8` | add / new / plus, confirm / check / apply, success                                                     |
| Red     | `#dc5f55` | `#ad433b` | `#f7dcd9` | delete, close, cancel, error, remove ×, destructive                                                    |
| Amber   | `#e9b664` | `#b5873a` | `#f7e2b8` | folders, clipboards, warnings, sun / light / brightness, wood (handles, pencils), sparkles              |
| Purple  | `#9a7ddc` | `#6f55b5` | `#e8e0f7` | effects and artistic accents, magic, flip/rotate mirror halves, paint colour                           |
| Teal    | `#13a3a3` | `#0b7a7a` | `#c6eeee` | secondary accent beside blue: plugins, secondary colour, network, history                              |

Three extra values:

- **Paper** `#fff`: documents and white canvas. Always bordered (§4).
- **Ink** `#2b2f36`: glyph marks that sit **on top of** a Paper, Tint or Amber fill (the warning
  "!", text on a page). Never put Ink directly on the transparent background.
- **Spectrum** for colour wheels and palettes only: Red, Amber, `#ffd43b` yellow, Green, Teal,
  Blue and Purple Bases.

### Rules

- Use at most three hues per icon (Paper and Ink don't count). Most icons use one hue plus Slate.
- Semantic colours win over literal ones. A delete badge is always Red, an add badge always Green,
  a selection always Blue.
- Keep Paint.NET's colour cue where it has a role in this table. Where Paint.NET's colour has no
  role (e.g. its purple floppy), use the role colour instead (our save is Blue).
- **Shading** is flat or a subtle two-tone, nothing more:
  - Base + Shade shows depth or a back plane: the folder back is Shade and the front is Base.
  - Base + Tint shows an interior or lit face: lens glass, picture sky.
  - Shade sits on bottom, right and back faces, and Tint or Paper on top, left and front faces.
  - A small Paper or Tint highlight stroke (e.g. the lens glint) is allowed.
  - Gradients (except the §2 motion-trail fade), opacity, glows and drop shadows are not.

## 6. Light and dark themes

One SVG must read on both the light theme (`#ffffff`) and the dark theme (`#202020`).
`render-check.sh` shows both, so check both every time.

- Every **Base** colour reaches at least 3:1 contrast against both backgrounds; Amber is the
  exception and reads by hue. Silhouette edges must be a Base (or Amber Base / Shade).
- **Tint and Paper** vanish on white, so they never form the outer edge. They always carry a
  1-px Base border, or sit fully inside a Base shape.
- **Shade** is weak on dark, so it never forms a large outer edge. Use it inside or behind a Base
  shape.
- **Ink** only sits on fills (§5).
- **Dark variants.** For Paint.NET's 11 `.dark` names, the base SVG already covers dark mode, so
  don't draw them. Only if an icon truly can't meet these rules, add `<name>.dark.svg`. App
  loaders resolve icons this way: in dark mode, use `<name>.dark.svg` if it exists, else
  `<name>.svg`. That's the same rule `crates/paintrs/src/icon_pack.rs` applies to the PNG packs,
  so `manifest.json` declares `"darkSuffix": ".dark"`. No `currentColor` and no runtime
  recolouring are involved.

## 7. Badges and overlays

Many Paint.NET icons are "object + small modifier". The modifier sits in the **badge zone**, the
bottom-right square 8–16 × 8–16 with its centre at 12,12. It is cut out of the object with a
**1-px gap**, done by a mask that draws the badge glyph dilated by 1 px. Copy this template and
change only the glyph `d`, the badge colour and the object:

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><defs><path id="b" d="GLYPH"/></defs><mask id="m"><rect width="16" height="16" fill="#fff"/><use href="#b" stroke="#000" stroke-width="2"/></mask><g mask="url(#m)">OBJECT</g><use href="#b" fill="BADGE"/></svg>
```

Standard glyphs. Reuse these exact paths; for new glyphs, keep the sizes.

| glyph   | colour | `d`                                                                                         |
|---------|--------|---------------------------------------------------------------------------------------------|
| plus    | Green  | `M11 9h2v2h2v2h-2v2h-2v-2H9v-2h2z` (2-px arms, 9–15)                                         |
| minus   | Red, or Slate for neutral "smaller" | `M9 11h6v2H9z`                                    |
| ×       | Red    | `M8.5 9.91 9.91 8.5 12 10.59 14.09 8.5 15.5 9.91 13.41 12 15.5 14.09 14.09 15.5 12 13.41 9.91 15.5 8.5 14.09 10.59 12z` (2-px arms, 8.5–15.5; the old 1.5-px arms went pink and mushy at 16 px) |
| 4-way   | Slate  | `M12 9h1v7h-1zM9 12h7v1H9zM11 10h3v1h-3zM11 14h3v1h-3zM10 11h1v3h-1zM14 11h1v3h-1z` (pixel-exact) |
| arrow   | Blue   | 2-px shaft plus a 45° filled head, about 6 px long inside the zone                           |
| status disc | Red / Amber / Blue | `<circle cx="12" cy="12" r="4"/>` with a 1-px Paper (or Ink on Amber) glyph inside |

Other badge rules:

- Badges are solid glyphs in a semantic colour. Don't use a Tint badge.
- When a badge carries a stroke (×, arrows), give the `<path id="b">` no colour attributes and put
  `fill`/`stroke` on each `<use>`. The mask `use` must stay pure black.
- **Top-right** holds state sparkles and stars (magic wand, auto-level) and needs no gap. Other
  corners are free for the object.
- If the badge would hide what identifies the object, shrink or shift the object, not the badge.
- Seeds that use badges: `menu_file_new_icon`, `menu_layers_add_new_layer_icon`,
  `menu_layers_delete_layer_icon` and `move_tool_icon`.

## 8. Shared motifs (keep identical across icons)

| motif          | snippet / seed                                                                                       |
|----------------|------------------------------------------------------------------------------------------------------|
| Document       | Paper body with a Slate border and a Slate-Tint dog-ear, see `menu_file_new_icon`: `M4.5 1.5h5l3 3v9a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1z` + fold `M9.5 1.5v3h3` |
| Sheet (copy/paste content) | Blue-Tint rect with a Blue border and Blue 1-px text lines, see `menu_edit_copy_icon` |
| Folder         | Amber-Shade back with tab, Amber front; open = slanted front, see `menu_file_open_icon`                |
| Clipboard      | Amber board (rx 1.5) with a Slate clip, see `menu_edit_paste_icon`                                     |
| Picture/image  | Blue-Tint frame with a Blue border, Blue mountains, Amber sun, see `anchor_chooser_control_anchor_image` |
| Layer          | Solid Blue tile, 10×10, rx 1.5. For stacks, draw the back tiles as Blue Tint with a Blue border     |
| Selection      | Blue-Tint fill with the Blue dashed marquee (§4), see `rectangle_select_tool_icon`                     |
| Pointer        | Blue filled arrow pointer, see `move_tool_icon`. A hollow variant (Tint fill, Blue border) is the "selection outline" version |
| Magnifier      | Blue 2-px ring, Blue-Tint glass, Slate handle, see `zoom_tool_icon`; add a Blue +/− inside for zoom in/out |
| Gear           | Slate dashed-circle teeth plus a ring, see `menu_utilities_settings_icon`                              |
| Undo / redo    | Blue 2-px U-turn with a filled head, see `menu_edit_undo_icon`; redo mirrors it                       |
| Disc status    | Red disc with a Paper × (`cancel_icon`); Blue disc with "i" (info); Amber triangle with an Ink "!" (`warning_icon`) |
| Text lines     | Slate 2-px pills with 1-px gaps, see `enum_text_alignment_left`                                        |
| Combine / boolean | Venn family, see §8a and `enum_selection_combine_mode_*` (all five done) |
| Mirror halves (flip/rotate) | Purple Base half plus Slate Tint half with a Slate border, split by a 1-px gap. Flips (image and layer alike): two 5×11 right triangles either side of the centre, see `menu_image_flip_horizontal_icon` |
| Rotate         | Purple triangle plus a Blue 2-px arc arrow: a quarter turn for 90°, a half turn for 180°, see `menu_image_rotate180_icon` |
| Tone wheels    | Spectrum pie, one segment per Spectrum colour, for colours (`menu_window_colors_icon`). Desaturate and sepia are 6-segment pies in Slate / Amber tones (Shade, Base, Tint, Paper, Tint, Base) with a 1-px Slate / Amber-Shade border ring |

## 8a. Combine and boolean icons: show the operands, fill the result

Any icon that shows a set operation follows this rule. That covers selection combine modes,
`menu_edit_paste_selection_*`, invert, select all / deselect, and any other "A op B".

1. **Operands** are always drawn as thin 1-px Slate marching-ants outlines (the §4 dashed
   marquee), whether or not they are part of the result. The reader must see both shapes.
2. The **result region** is filled solid Blue Base.
3. Where an operand outline crosses the filled result, it is redrawn in Blue Tint dashes (clipped
   to the result by a mask). That seam keeps "two shapes added together" readable.
4. The geometry is fixed. Back-left square A has its outline at 1.5–10.5 and interior 2–10.
   Front-right square B has its outline at 5.5–14.5 and interior 6–14. Both centre-line sides
   are 9, so the dashes hit every corner. The overlap interior is 6–10.

Template: change only `d` of `#r`, the result region. Interiors include the other square's
outline pixels, so the seams show.

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><defs><g id="o"><rect x="1.5" y="1.5" width="9" height="9"/><rect x="5.5" y="5.5" width="9" height="9"/></g><path id="r" d="RESULT"/></defs><mask id="m"><use href="#r" fill="#fff"/></mask><use href="#o" fill="none" stroke="#737a85" stroke-dasharray="1 2" stroke-linecap="square"/><use href="#r" fill="#5b95d6"/><use href="#o" fill="none" stroke="#d7e6f6" stroke-dasharray="1 2" stroke-linecap="square" mask="url(#m)"/></svg>
```

| mode      | result `d`                                                           |
|-----------|----------------------------------------------------------------------|
| replace   | `M6 6h8v8H6z` (B only)                                               |
| union     | `M2 2h8v4h4v8H6v-4H2z` (A ∪ B, seams visible)                         |
| exclude   | `M2 2h8v4H6v4H2z` (A − B)                                            |
| intersect | `M6 6h4v4H6z` (overlap only)                                         |
| xor       | `M2 2h8v4h4v8H6v-4H2zM6 6h4v4H6z` with `fill-rule="evenodd"` on `#r` |

- For `menu_edit_paste_selection_*`, reuse the matching result exactly and add a small Amber
  clipboard cue in the top-left. Let it overlap square A rather than moving the Venn pair. The cue
  is `<g id="c"><rect x="1" y="2" width="5" height="6" rx="1" fill="Amber"/><rect x="2" y="1"
  width="3" height="2" rx=".5" fill="Slate Shade"/></g>`. A second mask (white canvas, `#c` stroked
  2 px black) cuts it out of the whole Venn group with a 1-px gap, and `<use href="#c"/>` goes on
  top. Don't draw a full-size clipboard behind the pair: it was illegible at 16 px.
- An optional op badge (+ − ∩ ×) is allowed only if it stays legible at 16 px. The five combine
  modes read without one.
- For single-operand booleans (invert selection, select all), keep the same vocabulary: Slate
  marching ants for the shape, and a Blue fill for whatever the result covers. Invert fills the
  outside frame and leaves the shape empty: canvas 1–15 (rx 1.5) with the hole 4–12, and the
  shape's ants at 3.5–12.5 (side 9). They sit on the fill, so they are Blue-Tint seams.

## 9. Non-16 sizes

The viewBox equals Paint.NET's 96-DPI pixel size. Draw these on their own grid with the same
stroke weights; don't scale the strokes up.

| names                                                              | size  | note                                                                 |
|--------------------------------------------------------------------|-------|----------------------------------------------------------------------|
| `settings_*_24` (11)                                               | 24×24 | live area 2–22; template `settings_tools_24`                         |
| `enum_dash_style_*` (5)                                            | 28×16 | a dash sample line, 2 px tall, centred at y 8                        |
| `shapes_tool_icon`                                                 | 41×20 | the three-shape sample (square, circle, triangle)                    |
| `image_strip_close_button_*` (4)                                   | 17×17 | the normal/hot/pressed/disabled states of one × button              |
| `image_strip_scroll_left_arrow`, `_right_arrow`                    | 9×9   |                                                                      |
| `image_strip_dirty_overlay_11`                                     | 11×11 | the "unsaved" asterisk overlay                                       |
| `plugin_indicator`                                                 | 12×16 | gray plus: the mark for plugins and Extras; see §13                  |
| `tool_bar_image_list_menu_open_button`                             | 9×10  |                                                                      |
| `update_prompt_task_dialog_task_image`                             | 32×32 | live area 2–30                                                       |
| `pay_pal_donate`                                                   | 16×16 | **not** the PayPal button; draw a generic donate glyph (heart + coin) |
| `animation_busy_spinner`                                           | 16×16 | **one** frame (a ring with a gap); the app rotates it. Paint.NET's PNG is a 54-frame strip. Listed in `manifest.json` `singleFrame` |

`blank_icon` is intentionally empty and is already done.

## 10. Generic stand-ins (no third-party marks)

- `pay_pal_donate`, `menu_help_donate_icon`: a heart, or a heart with a coin.
- `enum_font_size_metric_fixed96_dpi`: Paint.NET shows an "f" with a Windows logo. Draw the
  letter shape as a path, plus a small monitor or pixel grid. Don't draw the Windows logo.
- `menu_help_pdn_website_icon`, `menu_help_about_icon`: a globe, and a Blue info disc. No
  Paint.NET logo.
- Letters, where one is needed (T, f, pt), are always drawn as paths, never as `<text>`.

## 11. Categories (batch units)

First match wins. Names with `.dark` use their base name. The page's `main.js` implements the same
rules.

1. `tools`: names containing `_tool_icon`, plus `paint_bucket_icon`.
2. `menu_file`, `menu_edit`, `menu_view`, `menu_image`: matched by prefix.
3. `menu_layers`: the `menu_layers_*` names plus `move_layer`.
4. `menu_other`: any other `menu_*` (help, window, utilities, tab).
5. `effects`: names ending `_effect`, `_effect_icon` or `_adjustment`, plus `auto_level` and
   `black_and_white_icon`.
6. `enum`: `enum_*`.
7. `settings`: `settings_*`.
8. `misc`: everything else (update, crash, image strip, tool-config strip, booleans, buttons).

## 12. Workflow per icon

1. Read the PNG for its concept, then close it.
2. Pick the keyline, motif and palette roles.
3. Write the SVG.
4. Run `./render-check.sh <names…>`. Fix every `WARN` line it prints (palette, forbidden
   features, size). Then open `out/sheet.png` (or `out/<name>.zoom.png`). Check:
   - the 16-px render is crisp on both tiles;
   - nothing vanishes on white or on dark;
   - it reads as the same concept as the PDN reference at the far left.
5. Check the page (`npm run dev`, port 5199) next to its neighbours, for family consistency.

## 13. New features (Photocraft port)

These cover the effects, adjustments and tools paint.rs adds beyond Paint.NET, taken from Photocraft.

- **Files.** The list is `photocraft-icons.txt` (`name|group|tier|label|concept`), and each icon goes
  to `assets/icons-svg/photocraft/<name>.svg`. `group` is the menu the item goes into. In the UI
  there is no "Photocraft" group.
- **No originals.** Draw from the `concept` field and this section. Like Paint.NET's effect icons,
  show a small sample of the effect's *result*. Never copy Photoshop's artwork.
- **No mark inside icons.** Never draw the plus mark into an icon; the app adds it.

### The plus mark

- **One gray plus** (user, 2026-10-24). `plugin_indicator.svg` marks plugins and Extras alike, in
  menus, tool options, dialog titles, the Layers window's T badge, the status bar and the Keyboard
  settings. It replaced both BB's puzzle piece and the gold `new_feature_indicator.svg`.
- **Slot.** 12×16, right-aligned 9 DIP from the edge and vertically centred in menus.
- **Shape.** A plus with 4-px arms, 10 px across (x 1–11, y 3–13): fill `#aaaaaa`, 1-px edge
  `#727272` (`stroke-linejoin="round"`). The fill carries it on dark backgrounds, the edge on light
  ones. It keeps 1 px clear left and right.
- **Small flat plus.** Where 10 px does not fit (the Tools window's 6-px corner plus, drawn by the
  app, and the §7 plus badge of BB's "+" plugins such as `hue_sat_plus`), use flat `#7d7d7d`.
- **Not the §7 badge.** Paint.NET's own green "new"/"add" badges (`menu_file_new_icon`,
  `menu_layers_add_new_layer_icon`, ...) stay green; they are not marks.

### Shared parts (copy exactly)

| part | snippet |
|------|---------|
| sketch sheet | `<rect x="1.5" y="1.5" width="13" height="13" rx="1" fill="#fff" stroke="#737a85"/>`; the subject is a disc, r 4.5 at 8,8 |
| texture tile | `<rect x="2" y="2" width="12" height="12" rx="1.5" fill="#5b95d6"/>` |
| pin-blur tile | `<rect x="1.5" y="1.5" width="13" height="13" rx="1" fill="#d7e6f6" stroke="#5b95d6"/>` |
| pin | `<circle r="1.75" fill="#fff" stroke="#3c74b4"/>`, always on Blue Base (a sharp area); on Tint it vanishes |
| dot grid | centres at 3, 8 and 13 on both axes; r goes from 2 down to 1, shrinking toward the bottom-right; r is never below .75 |
| auto / smart sparkle | `auto_level`'s Amber `M12 1Q12.84 3.16 15 4Q12.84 4.84 12 7Q11.16 4.84 9 4Q11.16 3.16 12 1z`, top-right, no gap (§7) |

### Families

- **Adjustments.** Use Paint.NET's adjustment vocabulary:
  - instruments: a Slate 1-px slider track with a Blue knob, histogram bars, a curve graph;
  - tone pies and swatches.
  - Auto Color and Auto Contrast add the auto sparkle.
  - Seed: `vibrance_adjustment`, a rainbow.
- **Blur Gallery** (Field, Iris, Path, Spin and Tilt-Shift, all in Blurs).
  - The pin-blur tile (Tint) is the blurred picture.
  - Blue Base marks what stays sharp. The shape of that sharp area is what identifies the item.
  - The pin sits on the sharp area.
  - Seed: `tilt_shift_effect`.
- **Brush Strokes.**
  - Strokes are free-standing: 2-px diagonals with round caps, no frame and no brush.
  - Blue and Purple stand for the image's two tones, with gaps of at least 1 px.
  - When two directions meet, stagger the rows by half a pitch. Joined strokes form chevrons, which
    read as back/forward arrows.
  - The stroke pattern is what identifies the item.
  - Seed: `angled_strokes_effect`.
- **Sketch.** A two-colour rendering.
  - The Paper sketch sheet is the secondary colour, and the subject disc drawn in Ink is the
    primary colour. This is Paint.NET's default black/white pair.
  - The mark style is what identifies the item: hatching, crayon dashes, a solid stamp, relief
    crescents.
  - Chrome and Water Paper ignore the colours. They keep the sheet but use no Ink.
  - Seed: `graphic_pen_effect`. Its hatch lines are 1.25 px, clipped to the disc by a mask, with
    gaps of at least 1 px.
- **Halftone.** A dot grid, with dot size following the tone.
  - Color Halftone uses rows of Amber, Purple and Teal as stand-ins for Y, M and C. Amber goes on the
    row of big dots, because small Amber dots vanish on white.
  - Halftone Pattern and Pointillize sit on the sketch sheet: primary colour on secondary.
  - Mezzotint drops the grid.
  - Seed: `color_halftone_effect`.
- **Texture.**
  - The texture tile is the image. The surface is carved into it with 1-px Blue Shade grooves, with
    no Tint highlight lines (§3).
  - Two exceptions: Craquelure is the one light (Tint) pattern, and Stained Glass uses coloured
    cells with Slate leading.
  - Seed: `texturizer_effect`, a brick pattern.
- **Artistic** (new items). Keep Paint.NET's medium metaphor (palette, pencil, pen): draw the tool or
  material together with its mark, with Purple as the paint.
- **Other existing groups** (Color, Distort, Noise, Object, Photo, Render, Stylize).
  - Match the Paint.NET icons in the same submenu: a Blue picture with a Purple accent.
  - Don't reuse a neighbour's exact motif. Where a concept names a neighbour to avoid, it says so.
- **Edit and Layers items** follow §8 and the `menu_edit_*` / `menu_layers_*` seeds.

### Tools

- **Object Selection** uses the §8a vocabulary.
  - The Slate marching-ants drag box is the operand. The object, a person bust filled solid Blue,
    is the result.
  - The box is the same 12-px box as `rectangle_select_tool_icon` (2.5–14.5).
- **Remove** is `eraser_tool_icon`'s eraser, shortened to the lower left, plus the sparkle at the
  top-right: it erases, then fills from the surroundings.
  - It is Eraser's sibling.
  - Fill Selection (Content-Aware) reuses its sparkle.
- **Healing Brush** is a bandage in Amber. Amber is the Clone Stamp's hue, and Healing Brush samples
  the way Clone Stamp does.
- **Clone Stamp's new options** need no icon. Sample reuses `sample_all_layers_*`, and Aligned is a
  text toggle.

## 14. Cursors

The canvas cursors replace paint.js's PNG cursors. Seeds: `paint_brush_tool_cursor` (crosshair and
glyph) and the `rectangle_select_tool_*` set (badges and mouse-down).

- **Files.** `assets/icons-svg/cursors/<name>.svg`, one per tool cursor name, root as §2 with
  `width="32" height="32" viewBox="0 0 32 32"`. The app rasterizes them at the device scale.
- **Hotspots.** `cursors/manifest.json` maps each name to `[x, y]` in the SVG's 32 × 32 units. Every
  cursor has an entry; the app scales it with the cursor.
- **Contrast.** An Ink (`#2b2f36`) core on a 1-px Paper (`#fff`) halo. Coloured glyphs keep their §5
  Base, Shade and Tint and also get the halo. The halo is the same geometry stroked 2 px wider and
  drawn underneath; no filters. Check each cursor on white, on `#202020` and on a busy image.
- **Crosshair.** 1-px Ink arms of 5 px at pixels 10–14 and 18–22 on row and column 16. The 3 × 3
  centre stays open, so the pixel under the hotspot (16, 16) stays visible.
- **Tool glyph.** A small version of the tool's own paint.rs icon in the lower-right box at about
  20–31, at least 1 px clear of the crosshair halo.
- **Badges.** Selection `plus` and `minus` variants put a §7 Green plus or Red minus (with a Paper
  halo) in the top-right zone, x 22–28, y 4–10.
- **Mouse-down.** `*_mouse_down_cursor` is the bare crosshair (no glyph), so the marquee stays
  uncovered while dragging.
- **Special shapes.** The text cursor is an Ink I-beam (hotspot mid-stem). The hands are flat Amber
  with an Amber Shade outline; the open and closed hands share one hotspot so the image doesn't
  jump on press or release. The clone source cursor is a ring with outside ticks and no inner cross.
