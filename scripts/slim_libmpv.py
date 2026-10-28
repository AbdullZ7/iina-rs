"""Trims shinchiro/mpv-winbuild-cmake's libmpv to what IINA-RS uses.

    python scripts/slim_libmpv.py <mpv-winbuild-cmake checkout>

Removed from FFmpeg: the video encoders x264, x265, libaom and SVT-AV1
(mpv only needs them for its --o recording mode; AV1 is decoded by dav1d)
and VapourSynth. Removed from mpv: the JavaScript engine (mujs), VapourSynth,
SDL gamepad input, Sixel terminal output and subrandr (YouTube SRV3
subtitles; the only Rust package, and Rust isn't in the build image). From
curl: HTTP/3 and ECH; curl and FFmpeg use Schannel for TLS instead of
OpenSSL. Lua, libarchive, Blu-ray/DVD, curl (HTTPS, HTTP/2), rubberband,
Vulkan/libplacebo and the decoders stay.

Each edit must match exactly: if the recipes change upstream, this fails
instead of silently building the full library.
"""

import pathlib
import sys

root = pathlib.Path(sys.argv[1]) / "packages"


def edit(name: str, drop_lines: list[str], replace: dict[str, str]) -> None:
    path = root / name
    # Bytes in and out: keeps the recipes' line endings.
    lines = path.read_bytes().decode("utf-8").split("\n")
    for d in drop_lines:
        hits = [i for i, l in enumerate(lines) if l.strip() == d]
        if len(hits) != 1:
            sys.exit(f"{name}: expected one line '{d}', found {len(hits)}")
        del lines[hits[0]]
    text = "\n".join(lines)
    for old, new in replace.items():
        if text.count(old) != 1:
            sys.exit(f"{name}: expected one '{old}', found {text.count(old)}")
        text = text.replace(old, new)
    path.write_bytes(text.encode("utf-8"))
    print(f"{name}: dropped {len(drop_lines)} lines, replaced {len(replace)}")


edit(
    "ffmpeg.cmake",
    [
        "x264",
        "${ffmpeg_x265}",
        "aom",
        "svtav1",
        "vapoursynth",
        "--enable-libx264",
        "--enable-libx265",
        "--enable-libaom",
        "--enable-libsvtav1",
        "--enable-vapoursynth",
    ],
    # TLS through Windows (Schannel): see curl below.
    {"--enable-openssl": "--enable-schannel"},
)

# The OpenSSL the recipes pin is a 4.1 development snapshot that current
# curl doesn't build against (no QUIC API, no SSL_set1_ech_config_list, no
# ASN1_STRING_get_length). curl and FFmpeg use Windows' own TLS (Schannel,
# the system certificate store) instead; libssh and libsrt keep OpenSSL.
# HTTP/3 and Encrypted Client Hello go too: no use for a player.
edit(
    "curl.cmake",
    ["ngtcp2", "nghttp3"],
    {
        "-DCURL_USE_OPENSSL=ON": "-DCURL_USE_OPENSSL=OFF\n        -DCURL_USE_SCHANNEL=ON",
        "-DUSE_NGHTTP3=ON": "-DUSE_NGHTTP3=OFF",
        "-DUSE_NGTCP2=ON": "-DUSE_NGTCP2=OFF",
        "-DUSE_PROXY_HTTP3=ON": "-DUSE_PROXY_HTTP3=OFF",
        "-DUSE_ECH=ON": "-DUSE_ECH=OFF",
        "-DUSE_SSLS_EXPORT=ON": "-DUSE_SSLS_EXPORT=OFF",
    },
)

edit(
    "mpv.cmake",
    ["mujs", "vapoursynth", "libsdl2", "libsixel", "subrandr"],
    {
        "-Djavascript=enabled": "-Djavascript=disabled",
        "-Dvapoursynth=enabled": "-Dvapoursynth=disabled",
        "-Dsdl2-gamepad=enabled": "-Dsdl2-gamepad=disabled",
        "-Dsixel=enabled": "-Dsixel=disabled",
        "-Dsubrandr=enabled": "-Dsubrandr=disabled",
    },
)
