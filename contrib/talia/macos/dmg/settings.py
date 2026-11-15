# dmgbuild settings for the Talia Desktop install window (decided 2026-11-14):
# the app on the left, the Applications folder on the right, large icons and
# an arrow between them on the background, so it is obvious to drag the app
# across. Used by contrib/talia/make-dmg.sh; `app` comes in with -D.
import os.path

HERE = defines["here"]  # noqa: F821 (dmgbuild); this folder, from make-dmg.sh
app = defines["app"]  # noqa: F821 (dmgbuild)
app_name = os.path.basename(app)

# make-dmg.sh builds read-write first, fixes the background link, then compresses.
format = defines.get("format", "UDZO")  # noqa: F821 (dmgbuild)
filesystem = "HFS+"
files = [app]
symlinks = {"Applications": "/Applications"}
icon = os.path.join(HERE, "..", "TaliaDesktop.icns")
# No hide_extensions: Finder hides ".app" anyway, and the flag it sets on the
# app (com.apple.FinderInfo) fails `codesign --verify --strict`.

# The background is 660 x 400 points (background.png, and background@2x.png
# for Retina screens). Icon centres match the arrow drawn on it.
background = os.path.join(HERE, "background.png")
window_rect = ((200, 140), (660, 400))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
icon_size = 128
text_size = 14
icon_locations = {
    app_name: (180, 200),
    "Applications": (480, 200),
}
