#!/bin/sh
# Copy the curated font set into $DEST (default /usr/share/fonts/pdf-engine) and, when
# fontconfig is installed, build its cache. Fails if any font is missing, so a package change
# can't silently drop weights or scripts.
# Needs Debian's fonts-noto-core, fonts-noto-extra, fonts-noto-mono, fonts-noto-cjk and
# fonts-noto-color-emoji packages.
set -eu
DEST=${DEST:-/usr/share/fonts/pdf-engine}
TTF=/usr/share/fonts/truetype/noto
OTF=/usr/share/fonts/opentype/noto
mkdir -p "$DEST"
missing=0
for f in \
    $TTF/NotoSans-Regular.ttf $TTF/NotoSans-Italic.ttf \
    $TTF/NotoSans-Light.ttf $TTF/NotoSans-LightItalic.ttf \
    $TTF/NotoSans-Medium.ttf $TTF/NotoSans-MediumItalic.ttf \
    $TTF/NotoSans-SemiBold.ttf $TTF/NotoSans-SemiBoldItalic.ttf \
    $TTF/NotoSans-Bold.ttf $TTF/NotoSans-BoldItalic.ttf \
    $TTF/NotoSans-ExtraBold.ttf $TTF/NotoSans-Black.ttf \
    $TTF/NotoSerif-Regular.ttf $TTF/NotoSerif-Italic.ttf \
    $TTF/NotoSerif-Bold.ttf $TTF/NotoSerif-BoldItalic.ttf \
    $TTF/NotoSansMono-Regular.ttf $TTF/NotoSansMono-Medium.ttf \
    $TTF/NotoSansMono-SemiBold.ttf $TTF/NotoSansMono-Bold.ttf \
    $TTF/NotoSansArabic-Regular.ttf $TTF/NotoSansArabic-Bold.ttf \
    $TTF/NotoNaskhArabic-Regular.ttf $TTF/NotoNaskhArabic-Bold.ttf \
    $TTF/NotoSansHebrew-Regular.ttf $TTF/NotoSansThai-Regular.ttf \
    $OTF/NotoSansCJK-Regular.ttc $OTF/NotoSansCJK-Bold.ttc \
    $TTF/NotoColorEmoji.ttf; do
    if [ -f "$f" ]; then cp -L "$f" "$DEST/"; else echo "missing font: $f" >&2; missing=1; fi
done
[ "$missing" = 0 ] || exit 1
if command -v fc-cache >/dev/null; then
    FONTCONFIG_FILE="$(dirname "$0")/fonts.conf" fc-cache -f
fi
echo "installed $(ls "$DEST" | wc -l) fonts into $DEST"
