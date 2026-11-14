"""One-time setup for the character pipeline (run by
`tools/scripts/build_characters.sh --setup`).

1. Downloads MPFB 2.0.18 from the Blender extensions platform and installs
   it into the *user* extension repository with Blender's own CLI
   (`blender --command extension install-file -r user_default -e`).
2. Downloads the MakeHuman system asset pack (CC0: skins, eyes, eyebrows,
   proxies...) and installs it with MPFB's own "load pack" operator into
   MPFB's user data folder.

Only your Blender user configuration is changed. Both downloads are checked
against the SHA-256 recorded here. Nothing needs a login.
"""

import hashlib
import os
import subprocess
import sys
import tempfile
import urllib.request

import bpy

MPFB_URL = (
    "https://extensions.blender.org/download/"
    "sha256:c075b3e9b94b8e5556a5f03f4653602d02ea306de08126e8f55ed6dee28a2dad/add-on-mpfb-v2.0.18.zip"
)
MPFB_SHA256 = "c075b3e9b94b8e5556a5f03f4653602d02ea306de08126e8f55ed6dee28a2dad"
ASSETS_URL = (
    "https://files.makehumancommunity.org/asset_packs/makehuman_system_assets/makehuman_system_assets_cc0.zip"
)
ASSETS_SHA256 = "b542127a8e25547c7c29c19f2d1d2adb9a664c80396ecd694095dbc8028a0107"

LOAD_PACK = """
import addon_utils, bpy, sys
name = next(m.__name__ for m in addon_utils.modules() if m.__name__.endswith('.mpfb'))
addon_utils.enable(name, default_set=True)
print(bpy.ops.mpfb.load_pack(filepath=sys.argv[-1]))
"""


def download(url, sha256, folder):
    path = os.path.join(folder, url.rsplit("/", 1)[-1])
    print(f"downloading {url}")
    with urllib.request.urlopen(url) as response, open(path, "wb") as out:
        while chunk := response.read(1 << 20):
            out.write(chunk)
    digest = hashlib.sha256(open(path, "rb").read()).hexdigest()
    if digest != sha256:
        sys.exit(f"{path}: SHA-256 {digest} does not match the expected {sha256}. "
                 "The upstream file changed; check it and update the hash in setup_mpfb.py.")
    return path


def main():
    blender = bpy.app.binary_path
    with tempfile.TemporaryDirectory(prefix="harrow_mpfb_") as folder:
        mpfb_zip = download(MPFB_URL, MPFB_SHA256, folder)
        subprocess.run([blender, "--command", "extension", "install-file", "-r", "user_default", "-e", mpfb_zip],
                       check=True)
        assets_zip = download(ASSETS_URL, ASSETS_SHA256, folder)
        subprocess.run([blender, "--background", "--factory-startup", "--python-expr", LOAD_PACK, "--", assets_zip],
                       check=True)
    print("MPFB and the MakeHuman CC0 system assets are installed.")


main()
