#!/usr/bin/env python3
"""Subset Noto Sans CJK SC for the Text fallback chain (R183 follow-up), offline.
Run: python3 -I scripts/fonts/subset_cjk.py /usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc
Writes assets/fonts/noto-sans-cjk-sc/NotoSansCJKsc-Regular.otf from TTC face 2 (SC): every
character of GB2312, JIS X 0208 and KS X 1001 (Python's codecs), all Hangul syllables and jamo,
Kana, CJK punctuation, enclosed/compatibility forms and half/fullwidth forms. GSUB/GPOS are
dropped: paint.rs draws cmap glyphs without shaping. The head timestamp is kept so the output is
reproducible with the fontTools version recorded in the provenance.
"""
import sys
from pathlib import Path
from fontTools import subset
from fontTools.ttLib import TTCollection

repo = Path(__file__).resolve().parents[2]
out = repo / 'assets/fonts/noto-sans-cjk-sc/NotoSansCJKsc-Regular.otf'


def charset(encoding):
    chars = set()
    for lead in range(0xA1, 0xFF):
        for trail in range(0xA1, 0xFF):
            try:
                chars.update(bytes([lead, trail]).decode(encoding))
            except UnicodeDecodeError:
                pass
    return {ord(c) for c in chars}


ranges = [(0x1100, 0x11FF), (0x2E80, 0x33FF), (0xA960, 0xA97F), (0xAC00, 0xD7FF),
          (0xFE30, 0xFE4F), (0xFF00, 0xFFEF)]
unicodes = {c for lo, hi in ranges for c in range(lo, hi + 1)}
for encoding in ['gb2312', 'euc_jp', 'euc_kr']:
    unicodes |= charset(encoding)
font = TTCollection(sys.argv[1]).fonts[2]
assert font['name'].getDebugName(4) == 'Noto Sans CJK SC'
options = subset.Options()
options.layout_features = []
options.drop_tables += ['GSUB', 'GPOS', 'GDEF', 'BASE']
options.name_IDs = ['*']
options.name_languages = ['*']
options.notdef_outline = True
subsetter = subset.Subsetter(options)
subsetter.populate(unicodes=unicodes)
subsetter.subset(font)
font.recalcTimestamp = False
out.parent.mkdir(parents=True, exist_ok=True)
font.save(out)
print(out, len(unicodes), len(font.getGlyphOrder()), out.stat().st_size)
