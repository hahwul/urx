#!/usr/bin/env bash
# Regenerates every urx brand asset from the single source illustration.
#
#   docs/tools/brand/src/mark-source.jpg  ->  docs/static/{images,icons}/*
#
# The source is a flat illustration on a near-black ground. The background is one
# connected region and the fuselage darks register as separate objects, so a corner
# floodfill cuts it cleanly without leaking into the plane. 14% is the usable
# ceiling: at 24% the fill breaks through the hull.
#
# Requires: ImageMagick 7 (`magick`).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STATIC="$(cd "$HERE/../.." && pwd)/static"
SRC="$HERE/src/mark-source.jpg"
# Space Grotesk ships variable-only; its default instance is 300 (Light), so the
# static weights are instantiated by fonts/build-static.sh and committed alongside.
FONT_BOLD="$HERE/fonts/SpaceGrotesk-Bold.ttf"
FONT_MED="$HERE/fonts/SpaceGrotesk-Medium.ttf"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

INK="#0B0B0E"        # dark surface, matches --bg dark
AMBER="#FCA428"      # accent, sampled from the thruster ring
STEEL="#9AA3B4"      # light-mode highlight stroke replacement

mkdir -p "$STATIC/images" "$STATIC/icons"

# ---------------------------------------------------------------- 1. cut the mark
magick "$SRC" -alpha off \
  -bordercolor black -border 1 \
  -fuzz 14% -fill none -floodfill +0+0 black \
  -shave 1x1 -trim +repage \
  "$TMP/mark-raw.png"

# Dark-background variant: the artwork as drawn, white highlight strokes intact.
magick "$TMP/mark-raw.png" -resize 1024x "$STATIC/images/mark.png"

# Light-background variant: the near-white strokes vanish against a light page and
# split the silhouette, so drop them to a mid steel that still reads as a highlight.
magick "$TMP/mark-raw.png" -fuzz 14% -fill "$STEEL" -opaque "#F9F9FA" \
  -resize 1024x "$STATIC/images/mark-light.png"

magick "$STATIC/images/mark.png"       -quality 92 "$STATIC/images/mark.webp"
magick "$STATIC/images/mark-light.png" -quality 92 "$STATIC/images/mark-light.webp"

# ------------------------------------------------------------------- 2. icon tiles
# The mark is 1.68:1 landscape. Fitted bare into a square it renders ~40% smaller
# and turns to mush at 16px, so the icon rotates the jet onto the square's diagonal
# and brightens the hull, which is otherwise near-black against a near-black tile.
# An amber bloom sits at the thruster: at 16px colour is the recognition cue, not form.
magick "$TMP/mark-raw.png" -background none -rotate -32 -trim +repage \
  -modulate 165 "$TMP/mark-icon.png"

#   $1 = output  $2 = size  $3 = mark size as % of tile  $4 = corner radius %
tile() {
  local out=$1 size=$2 pct=$3 radius=$4
  local mw=$(( size * pct / 100 ))
  local r=$(( size * radius / 100 ))

  magick -size "${size}x${size}" xc:none -fill "$AMBER" \
    -draw "circle $(( size * 36 / 100 )),$(( size * 68 / 100 )) $(( size * 36 / 100 )),$(( size * 50 / 100 ))" \
    -blur "0x$(( size / 9 + 1 ))" "$TMP/bloom.png"

  magick -size "${size}x${size}" "xc:$INK" \
    \( "$TMP/bloom.png" -evaluate multiply 0.42 \) -compose over -composite \
    \( "$TMP/mark-icon.png" -resize "${mw}x${mw}" \) -gravity center -composite \
    "$TMP/tile.png"

  if [ "$r" -gt 0 ]; then
    magick -size "${size}x${size}" xc:none \
      -draw "roundrectangle 0,0 $((size-1)),$((size-1)) $r,$r" \
      -alpha extract "$TMP/mask.png"
    magick "$TMP/tile.png" "$TMP/mask.png" \
      -alpha off -compose CopyOpacity -composite "$out"
  else
    cp "$TMP/tile.png" "$out"
  fi
}

tile "$STATIC/icons/favicon-96x96.png"             96  88 18
tile "$TMP/fav-16.png"                             16  90 18
tile "$TMP/fav-32.png"                             32  90 18
tile "$TMP/fav-48.png"                             48  88 18
magick "$TMP/fav-16.png" "$TMP/fav-32.png" "$TMP/fav-48.png" "$STATIC/icons/favicon.ico"

# iOS applies its own rounding, so ship a square tile.
tile "$STATIC/icons/apple-touch-icon.png"         180  80  0
# Maskable icons are cropped to a centre circle; keep the jet inside the safe zone.
tile "$STATIC/icons/web-app-manifest-192x192.png" 192  62  0
tile "$STATIC/icons/web-app-manifest-512x512.png" 512  62  0

# ---------------------------------------------------------------- 3. README lockups
#   $1 = output  $2 = text colour  $3 = mark variant
lockup() {
  local out=$1 fg=$2 mark=$3
  magick -size 800x321 xc:none \
    \( "$mark" -resize 340x \) -gravity west -geometry +48+0 -composite \
    \( -background none -fill "$fg" -font "$FONT_BOLD" -pointsize 150 \
       -kerning -4 label:"urx" \) -gravity west -geometry +430+0 -composite \
    "$out"
}
lockup "$STATIC/images/urx-dark.png"  "#F9F9FA" "$STATIC/images/mark.png"
lockup "$STATIC/images/urx-light.png" "#16181D" "$STATIC/images/mark-light.png"

# -------------------------------------------------------------------- 4. OG card
# 1200x630. Replaces preview.jpg, which was 819 KB at 3840x2433.
# A tight amber bloom sits behind the engine, where the thruster actually is. A
# full-bleed amber wash over the whole card just reads as muddy brown.
magick -size 1200x630 xc:none -fill "$AMBER" \
  -draw "circle 668,330 668,150" -blur 0x110 "$TMP/glow.png"

magick -size 1200x630 "xc:$INK" \
  \( "$TMP/glow.png" -evaluate multiply 0.55 \) -compose over -composite \
  \( "$STATIC/images/mark.png" -resize 520x \) -gravity northeast -geometry +72+110 -composite \
  -font "$FONT_BOLD" -fill "#F9F9FA" -pointsize 138 -kerning -4 \
    -gravity northwest -annotate +80+286 "urx" \
  -fill "$AMBER" -draw "rectangle 82,452 214,456" \
  -font "$FONT_MED" -fill "#A2A8B8" -pointsize 34 -interline-spacing 12 \
    -gravity northwest -annotate +80+492 "Extracts URLs from OSINT archives\nfor security insights." \
  "$STATIC/images/og-card.png"

# ------------------------------------------------------------------- 5. optimise
# Everything here is flat vector-style art, so a 256-colour palette is visually
# lossless (measured RMSE < 1%) and cuts the PNGs by 3-4x. No pngquant dependency.
for f in "$STATIC/images/mark.png" "$STATIC/images/mark-light.png" \
         "$STATIC/images/urx-dark.png" "$STATIC/images/urx-light.png" \
         "$STATIC/images/og-card.png" \
         "$STATIC/icons/favicon-96x96.png" "$STATIC/icons/apple-touch-icon.png" \
         "$STATIC/icons/web-app-manifest-192x192.png" \
         "$STATIC/icons/web-app-manifest-512x512.png"; do
  magick "$f" -strip -colors 256 -define png:compression-level=9 "$f"
done

echo "brand assets written to $STATIC"
find "$STATIC/images" "$STATIC/icons" -newermt '-2 minutes' -type f | sort | sed 's|.*/static/|  static/|'
