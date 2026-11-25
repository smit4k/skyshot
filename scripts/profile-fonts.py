"""Generate the profile text optical sizes from the bundled Inter source.

Requires fonttools 4.68.0. Normal builds use the checked-in font files.
"""

from pathlib import Path

from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont


root = Path(__file__).resolve().parent.parent / "fonts"
for suffix, size, weight in (("Text", 15, None), ("Compact", 24.3, 700), ("Display", 30, 700)):
    family = f"Skyshot Profile {suffix}"
    axes = {"opsz": size}
    if weight:
        axes["wght"] = weight
    font = instantiateVariableFont(
        TTFont(root / "InterVariable.ttf", recalcTimestamp=False),
        axes,
        inplace=True,
    )
    names = font["name"]
    for entry in list(names.names):
        if entry.nameID in (1, 3, 4, 6, 16, 21, 25):
            value = family.replace(" ", "") if entry.nameID in (6, 25) else family
            names.setName(value, entry.nameID, entry.platformID, entry.platEncID, entry.langID)
    font.save(root / f"Profile{suffix}.ttf")
