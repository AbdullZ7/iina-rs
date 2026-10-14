#!/usr/bin/env python3
"""Package IINA-RS release builds.

  package.py windows <iina-rs.exe> <libmpv-2.dll> <out_dir>
  package.py linux   <iina-rs>                    <out_dir>

Windows -> portable .zip (exe + libmpv-2.dll).
Linux   -> .tar.gz (binary, .desktop, icon, install.sh) and a .deb that
           depends on the distro's libmpv.
"""

import io
import os
import sys
import tarfile
import time
import re
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
with open(os.path.join(ROOT, "Cargo.toml"), encoding="utf-8") as f:
    VERSION = re.search(r'^version\s*=\s*"([^"]+)"', f.read(), re.M).group(1)

MIME_TYPES = ";".join([
    "video/mp4", "video/x-matroska", "video/webm", "video/x-msvideo", "video/quicktime", "video/mpeg",
    "video/x-flv", "video/x-ms-wmv", "video/ogg", "video/3gpp", "video/mp2t", "audio/mpeg", "audio/flac",
    "audio/mp4", "audio/ogg", "audio/x-wav", "audio/aac", "audio/opus", "application/x-mpegurl",
]) + ";"

DESKTOP = f"""[Desktop Entry]
Type=Application
Name=IINA-RS
GenericName=Media Player
Comment=Modern media player inspired by IINA, powered by mpv
Exec=iina-rs %U
Icon=iina-rs
Terminal=false
Categories=AudioVideo;Video;Audio;Player;
MimeType={MIME_TYPES}
"""

INSTALL_SH = """#!/bin/sh
# Installs IINA-RS for the current user (or system-wide when run as root).
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
if [ "$(id -u)" = 0 ]; then PREFIX=/usr/local; else PREFIX="$HOME/.local"; fi
install -Dm755 "$HERE/iina-rs" "$PREFIX/bin/iina-rs"
install -Dm644 "$HERE/iina-rs.desktop" "$PREFIX/share/applications/iina-rs.desktop"
install -Dm644 "$HERE/iina-rs.png" "$PREFIX/share/icons/hicolor/256x256/apps/iina-rs.png"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
echo "Installed to $PREFIX/bin/iina-rs"
if ! ldconfig -p 2>/dev/null | grep -q 'libmpv\\.so'; then
  echo "NOTE: libmpv not found. Install it, e.g.:"
  echo "  Debian/Ubuntu: sudo apt install libmpv2   (older: libmpv1)"
  echo "  Fedora:        sudo dnf install mpv-libs"
  echo "  Arch:          sudo pacman -S mpv"
fi
"""


def read(path):
    with open(path, "rb") as f:
        return f.read()


def tar_add(tar, name, data, mode=0o644):
    info = tarfile.TarInfo(name)
    info.size = len(data)
    info.mode = mode
    info.mtime = int(time.time())
    info.uname = info.gname = "root"
    tar.addfile(info, io.BytesIO(data))


def tar_dir(tar, name):
    info = tarfile.TarInfo(name)
    info.type = tarfile.DIRTYPE
    info.mode = 0o755
    info.mtime = int(time.time())
    info.uname = info.gname = "root"
    tar.addfile(info)


def docs():
    return {
        "README.md": read(os.path.join(ROOT, "README.md")),
        "LICENSE": read(os.path.join(ROOT, "LICENSE")),
    }


def package_windows(exe, dll, out):
    name = f"IINA-RS-{VERSION}-windows-x64"
    path = os.path.join(out, name + ".zip")
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.write(exe, f"{name}/iina-rs.exe")
        z.write(dll, f"{name}/libmpv-2.dll")
        for fname, data in docs().items():
            z.writestr(f"{name}/{fname}", data)
    print(path)


def package_linux(binary, out):
    name = f"IINA-RS-{VERSION}-linux-x86_64"
    files = {
        "iina-rs": (read(binary), 0o755),
        "iina-rs.desktop": (DESKTOP.encode(), 0o644),
        "iina-rs.png": (read(os.path.join(ROOT, "assets", "iina-rs.png")), 0o644),
        "install.sh": (INSTALL_SH.encode(), 0o755),
    }
    for fname, data in docs().items():
        files[fname] = (data, 0o644)
    tgz = os.path.join(out, name + ".tar.gz")
    with tarfile.open(tgz, "w:gz") as tar:
        tar_dir(tar, name)
        for fname, (data, mode) in files.items():
            tar_add(tar, f"{name}/{fname}", data, mode)
    print(tgz)
    print(build_deb(binary, out))


def build_deb(binary, out):
    """Builds a .deb by hand (ar + tar) so it can be produced on any OS."""
    data_files = [
        ("./usr/bin/iina-rs", read(binary), 0o755),
        ("./usr/share/applications/iina-rs.desktop", DESKTOP.encode(), 0o644),
        ("./usr/share/icons/hicolor/256x256/apps/iina-rs.png", read(os.path.join(ROOT, "assets", "iina-rs.png")), 0o644),
        ("./usr/share/doc/iina-rs/copyright", read(os.path.join(ROOT, "LICENSE")), 0o644),
    ]
    data_buf = io.BytesIO()
    with tarfile.open(fileobj=data_buf, mode="w:gz") as tar:
        dirs = set()
        for path, _, _ in data_files:
            parts = path.split("/")[:-1]
            for i in range(1, len(parts) + 1):
                dirs.add("/".join(parts[:i]))
        for d in sorted(dirs):
            tar_dir(tar, d + "/")
        for path, data, mode in data_files:
            tar_add(tar, path, data, mode)
    installed_kb = sum(len(d) for _, d, _ in data_files) // 1024
    control = f"""Package: iina-rs
Version: {VERSION}
Section: video
Priority: optional
Architecture: amd64
Depends: libc6 (>= 2.31), libmpv2 | libmpv1
Recommends: yt-dlp, libgl1, libegl1, libxkbcommon0
Installed-Size: {installed_kb}
Maintainer: IINA-RS contributors <noreply@example.invalid>
Homepage: https://github.com/iina/iina
Description: Modern media player inspired by IINA, powered by mpv
 IINA-RS brings the look and feel of the macOS player IINA (floating
 on-screen controller, playlist and quick-settings sidebar, picture-in-
 picture, music mode, IINA key bindings) to Linux and Windows.
"""
    ctrl_buf = io.BytesIO()
    with tarfile.open(fileobj=ctrl_buf, mode="w:gz") as tar:
        tar_add(tar, "./control", control.encode())
        postinst = b"#!/bin/sh\nset -e\ncommand -v update-desktop-database >/dev/null && update-desktop-database -q || true\n"
        tar_add(tar, "./postinst", postinst, 0o755)

    def ar_member(name, data):
        hdr = f"{name:<16}{int(time.time()):<12}{0:<6}{0:<6}{'100644':<8}{len(data):<10}`\n".encode()
        return hdr + data + (b"\n" if len(data) % 2 else b"")

    deb = b"!<arch>\n"
    deb += ar_member("debian-binary", b"2.0\n")
    deb += ar_member("control.tar.gz", ctrl_buf.getvalue())
    deb += ar_member("data.tar.gz", data_buf.getvalue())
    path = os.path.join(out, f"iina-rs_{VERSION}_amd64.deb")
    with open(path, "wb") as f:
        f.write(deb)
    return path


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    kind = sys.argv[1]
    if kind == "windows" and len(sys.argv) == 5:
        os.makedirs(sys.argv[4], exist_ok=True)
        package_windows(sys.argv[2], sys.argv[3], sys.argv[4])
    elif kind == "linux" and len(sys.argv) == 4:
        os.makedirs(sys.argv[3], exist_ok=True)
        package_linux(sys.argv[2], sys.argv[3])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
