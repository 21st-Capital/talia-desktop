"""Give the install window's background a bookmark macOS 26 accepts.

dmgbuild writes the background reference itself (mac_alias), and Finder on
macOS 26 ignores it: the window opens without its picture (found 2026-11-14,
while a DMG built by Finder showed its own). Here macOS makes the bookmark
(NSURL bookmarkData, through JavaScript for Automation; Foundation only, no
Finder scripting) and it replaces the 'pBBk' entry of the volume's .DS_Store.

    python background_bookmark.py <mounted read-write volume>
"""
import subprocess
import sys
import tempfile
from pathlib import Path

import ds_store.store as store
from ds_store import DSStore

JXA = """
ObjC.import("Foundation");
function run(argv) {
  var url = $.NSURL.fileURLWithPath(argv[0]);
  var data = url.bookmarkDataWithOptionsIncludingResourceValuesForKeysRelativeToURLError(0, $(), $(), Ref());
  if (data.isNil()) { throw new Error("no bookmark for " + argv[0]); }
  data.writeToFileAtomically(argv[1], true);
  return "ok";
}
"""


def main(volume: Path) -> None:
    pictures = sorted(volume.glob(".background*"))
    pictures = [p for p in pictures if p.is_file()] or sorted((volume / ".background").glob("*"))
    if len(pictures) != 1:
        raise SystemExit(f"expected one background picture on {volume}, found {pictures}")
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "bookmark"
        subprocess.run(["osascript", "-l", "JavaScript", "-e", JXA, str(pictures[0]), str(out)],
                       check=True, capture_output=True)
        bookmark = out.read_bytes()
    # Write the bytes as they are, not through mac_alias's Bookmark codec.
    store.codecs.pop(b"pBBk", None)
    with DSStore.open(str(volume / ".DS_Store"), "r+") as d:
        try:
            del d["."]["pBBk"]
        except KeyError:
            pass
        d["."]["pBBk"] = ("blob", bookmark)
    print(f"background bookmark: {pictures[0].name}, {len(bookmark)} bytes")


if __name__ == "__main__":
    main(Path(sys.argv[1]))
