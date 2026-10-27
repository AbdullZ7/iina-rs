# IINA-RS

**[العربية](#العربية) · [English](#english)**

---

## العربية

مشغّل وسائط لـ **ويندوز** و**لينكس** و**ماك** بروح [IINA](https://github.com/iina/iina)، مكتوب بـ **Rust** ومبني على **libmpv** ([mpv.io](https://mpv.io))، مع ميزات مأخوذة من PotPlayer.

[التنزيل من صفحة الإصدارات](https://github.com/AbdullZ7/iina-rs/releases/latest)

### لماذا إعادة كتابة وليس تحويلاً؟

تحليل `IINA.v1.5.0.dmg`:

| المكوّن | المحتوى |
|---|---|
| `MacOS/IINA` | Mach-O universal (x86_64 + arm64)، مكتوب بـ Swift + AppKit (واجهة ماك فقط) |
| `Frameworks/` | `libmpv.2.dylib` + FFmpeg + libass وlibplacebo وdav1d… + Sparkle (التحديثات) |
| `MacOS/youtube-dl` | yt-dlp مدمج |
| `Resources/config/*.conf` | اختصارات لوحة المفاتيح (صيغة mpv) |
| `Resources/plugins/` | إضافات JS (opensub وytdl وuserscript) |

الملفات التنفيذية خاصة بماك والواجهة مبنية على AppKit، فلا تعمل على ويندوز أو لينكس. لكن قلب التشغيل (mpv) يعمل على كل الأنظمة، فاحتفظ IINA-RS بنفس القلب، واستعمل ملفات اختصارات IINA وأيقونته (GPLv3)، وأعاد بناء واجهة IINA بلغة Rust.

### الميزات

**التشغيل والعرض**
- تشغيل بـ libmpv مع فك الترميز بالعتاد (d3d11va / vaapi / nvdec / videotoolbox).
- **خَرْج HDR حقيقي عبر Vulkan** على الشاشات الداعمة (ويندوز، ولينكس على Wayland) بصيغة HDR10، مع رجوع آلي إلى OpenGL. والفيديو العادي (SDR) يُعرض على شاشات HDR مثل ويندوز تماماً.
- واجهة سريعة الاستجابة: لا تنتظر إطار الفيديو التالي، فتتحدّث بسرعة الشاشة.
- استئناف التشغيل من آخر موضع، والملفات الأخيرة، وإضافة باقي ملفات المجلد لقائمة التشغيل بترتيب طبيعي (ep2 قبل ep10).
- فتح المسارات (يوتيوب والمواقع الأخرى عبر yt-dlp)، والسحب والإفلات.

**الواجهة (بأسلوب IINA)**
- شريط تحكم عائم يختفي آلياً، أو شريط في الأسفل أو الأعلى؛ وشريط وقت بالفصول والتخزين المؤقت وتكرار A-B.
- **مصغّرات على شريط الوقت** (480 بكسل، واحدة كل 4 ثوانٍ تقريباً) مع ذاكرة على القرص.
- رسائل على الشاشة بأسلوب IINA بدل واجهة mpv.
- شريط جانبي: قائمة التشغيل، الفصول، إعدادات المرئيات والسمعيات والترجمة.
- معادِل الصورة، ومعادِل صوت بعشرة نطاقات، والقلب والمرآة والدوران والاقتصاص والنسبة الباعيّة.
- صورة داخل صورة (تُسحب وتُكبَّر وتحافظ على أبعاد الفيديو)، ووضع موسيقى كامل (الغلاف والعنوان والفنان وقائمة التشغيل، آلياً للملفات السمعية)، وفوق كل النوافذ.
- المفتّش، والقفز إلى وقت، وتاريخ التشغيل، ونافذة المرشّحات، ونافذة السجلات، ومحرّر ارتباطات المفاتيح.
- تفضيلات بستة أقسام مثل IINA: عام، المرئيات والسمعيات، الترجمات، الشبكة، التحكم، متقدم.

**الترجمة**
- البحث عن ترجمات على الإنترنت وتنزيلها من **SubDL** (المبدئي) و**OpenSubtitles**، مع التعرّف على الموسم والحلقة.
- إعدادات بأسلوب PotPlayer، ونافذة خطوط مع **عيّنة حيّة يرسمها libass** فتطابق الترجمة على الفيديو.
- حجم الترجمة يتبع الفيديو، وترتفع بنعومة فوق شريط التحكم.
- ترجمة ثانوية، والبحث في المجلدات الفرعية (`Subs/` و`Subtitles/`)، وثلاث خطوات مزامنة.

**من PotPlayer**
- اختصاراته الافتراضية، ومفاتيح حجم النافذة (` و1–8، وCtrl+Alt+لوحة الأرقام).
- سبعة أزرار تحت صورة البرنامج في شريط المهام (ويندوز).
- رفع الصوت حتى 150% (و200% من التفضيلات) وتسوية الصوت (Shift+N).

**النظام**
- أزرار الوسائط ولوحة الوسائط في ويندوز وشاشة القفل، وMPRIS على لينكس، وNow Playing على ماك.
- نسخة واحدة، وإضافة البرنامج لقائمة «فتح باستخدام» في ويندوز، ومنع الجهاز من النوم أثناء التشغيل.
- الاختصارات تعمل والكيبورد على العربي، ولوحة اللمس تتبع الأصابع، والزرّان الجانبيان للماوس قابلان للتخصيص.
- سكربتات mpv (Lua / JS) من مجلد `scripts`، والتحقق من التحديثات (مرة يومياً، ويمكن إيقافه).

**واجهة عربية كاملة** من اليمين لليسار بمصطلحات قاموس عرب آيز التقني، مع وصل الحروف وعرض النص باتجاهه الصحيح في كل مكان. تُختار من التفضيلات: آلي (حسب لغة النظام) أو English أو العربية.

**غير متوفر مقارنةً بـ IINA:** إضافات IINA بـ JavaScript (مبنية على واجهات ماك؛ البديل سكربتات mpv)، وLive Text (يعتمد على Apple Vision).

### التثبيت

**ويندوز:** شغّل المثبّت `IINA-RS-x.y.z-windows-x64-setup.exe`: يثبّت البرنامج لحسابك دون صلاحيات المدير (أو لكل المستخدمين)، ويضيف اختصاراً في قائمة ابدأ، ويعرض إضافته إلى «فتح باستخدام»، ويُزال من إعدادات ويندوز. أو استعمل النسخة المحمولة: فكّ ضغط `IINA-RS-x.y.z-windows-x64.zip` وشغّل `iina-rs.exe` (معه `libmpv-2.dll`). لروابط يوتيوب ضع `yt-dlp.exe` بجانب البرنامج أو في PATH.

**لينكس:**
```bash
sudo apt install ./iina-rs_0.5.1_amd64.deb
```
أو من الأرشيف:
```bash
tar xf IINA-RS-0.5.1-linux-x86_64.tar.gz && ./IINA-RS-0.5.1-linux-x86_64/install.sh
```
يتطلب libmpv: `libmpv2` (Debian/Ubuntu) أو `mpv-libs` (Fedora) أو `mpv` (Arch). الملف التنفيذي لا يحتاج إلا glibc ≥ 2.31، وتُحمَّل libmpv وOpenGL وX11/Wayland وقت التشغيل.

**ماك (Apple Silicon):** افتح `IINA-RS-x.y.z-macos-arm64.dmg` واسحب البرنامج إلى Applications. النسخة غير موقّعة من Apple، فانقر عليها بالزر الأيمن ثم «فتح» في أول مرة.

### الاختصارات

| المفتاح | الوظيفة |
|---|---|
| Space | تشغيل / توقف مؤقت |
| ← / → | سعي ±5 ثوانٍ |
| ↑ / ↓ | حجم الصوت ±5 |
| Ctrl+O / Ctrl+U | فتح ملف / مسار |
| Ctrl+Shift+P | قائمة التشغيل |
| Ctrl+Shift+V / A / S | إعدادات المرئيات / السمعيات / الترجمة |
| f، F11، Enter / Esc | ملء الشاشة / خروج |
| Ctrl+Alt+P / T / M | صورة داخل صورة / فوق كل النوافذ / وضع الموسيقى |
| Ctrl+S | لقطة شاشة |
| Z / X | تأخير الترجمة |
| ` و1–8 | حجم النافذة (مثل PotPlayer): الأصغر، ‎0.5×–2×، الأكبر، تكبير، ملء الشاشة، ملاءمة النسبة |
| Ctrl+I / Ctrl+J / Ctrl+H | المفتّش / القفز إلى وقت / تاريخ التشغيل |
| Ctrl+, | التفضيلات |
| Ctrl+Shift+D | البحث عن ترجمات على الإنترنت |

تُعدَّل الاختصارات من التفضيلات ← التحكم ← «حرِّر ارتباطات المفاتيح…»، وتُحفظ في `input.conf` (صيغة mpv / IINA) داخل مجلد الإعدادات: ويندوز `%APPDATA%\iina-rs\`، لينكس `~/.config/iina-rs/`. ويُقرأ `mpv.conf` من المجلد نفسه (لا تُقرأ إعدادات mpv العامة).

### البناء

```bash
cargo build --release
# ملف لينكس من ويندوز أو ماك:
pip install ziglang && cargo install cargo-zigbuild
cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.31
# الحزم:
python scripts/package.py windows target/release/iina-rs.exe path/to/libmpv-2.dll dist
python scripts/package.py linux target/x86_64-unknown-linux-gnu/release/iina-rs dist
```
ملف GitHub Actions في `.github/workflows/build.yml` يبني الأنظمة الثلاثة، وينشر إصداراً عند رفع وسم `v*`، ويأخذ وصفه من `docs/releases/<الوسم>.md`.

### الترخيص

GPL-3.0-or-later مثل IINA. ملفات الاختصارات وأيقونة البرنامج من IINA (© مساهمو IINA)، والتشغيل يوفّره mpv.

---

## English

A media player for **Windows**, **Linux** and **macOS** in the spirit of [IINA](https://github.com/iina/iina), written in **Rust** on top of **libmpv** ([mpv.io](https://mpv.io)), with a few favourites borrowed from PotPlayer.

[Download from the releases page](https://github.com/AbdullZ7/iina-rs/releases/latest)

### Why a rewrite instead of a port?

Looking inside `IINA.v1.5.0.dmg`:

| Part | Contents |
|---|---|
| `MacOS/IINA` | Mach-O universal (x86_64 + arm64), Swift + AppKit (macOS-only UI) |
| `Frameworks/` | `libmpv.2.dylib` + FFmpeg + libass, libplacebo, dav1d… + Sparkle (updates) |
| `MacOS/youtube-dl` | bundled yt-dlp |
| `Resources/config/*.conf` | key bindings (mpv format) |
| `Resources/plugins/` | JS plugins (opensub, ytdl, userscript) |

The binaries are Mach-O and the UI is AppKit, so they can't run on Windows or Linux. The playback core (mpv) is cross-platform, though: IINA-RS keeps that core, reuses IINA's key-binding files and icon (GPLv3), and rebuilds IINA's UI in Rust.

### Features

**Playback & display**
- libmpv playback with hardware decoding (d3d11va / vaapi / nvdec / videotoolbox).
- **Real HDR output via Vulkan** on HDR displays (Windows, and Linux on Wayland) as HDR10, with automatic OpenGL fallback; SDR video on an HDR display looks exactly as Windows shows it.
- A responsive UI that never waits for the next video frame: it updates at the display's refresh rate.
- Resume where you left off, recent files, and the rest of the folder added to the playlist in natural order (ep2 before ep10).
- Open URLs (YouTube and other sites via yt-dlp), drag & drop.

**Interface (IINA-style)**
- Floating auto-hiding controller, or a bar at the bottom or top; seek bar with chapters, cache and A-B loop.
- **Seek-bar thumbnails** (480 px, about one every 4 s) with an on-disk cache.
- IINA-style on-screen messages instead of mpv's OSD.
- Sidebar: playlist, chapters, video / audio / subtitle settings.
- Video equalizer, 10-band audio equalizer, flip / mirror / rotate / crop / aspect ratio.
- Picture-in-Picture (movable, resizable, keeps the video's aspect), full music mode (cover art, title, artist, playlist; automatic for audio files), float on top.
- Inspector, jump to time, playback history, filters window, logs window, key-binding editor.
- Six-tab preferences like IINA: General, Video & Audio, Subtitles, Network, Controls, Advanced.

**Subtitles**
- Search and download subtitles online from **SubDL** (default) and **OpenSubtitles**, with season / episode detection.
- PotPlayer-style settings and a font dialog with a **live sample rendered by libass**, matching the subtitle on the video.
- Subtitle size follows the video; subtitles glide just above the controller.
- Secondary subtitles, subtitles from subfolders (`Subs/`, `Subtitles/`), three sync steps.

**From PotPlayer**
- Its default shortcuts and window-size keys (` and 1–8, Ctrl+Alt+Numpad).
- Seven taskbar thumbnail buttons (Windows).
- Volume up to 150 % (200 % in settings) and audio normalization (Shift+N).

**System**
- Media keys, the Windows media flyout and lock screen (SMTC), MPRIS on Linux, Now Playing on macOS.
- Single instance, Windows "Open with" registration, keeps the display awake while playing.
- Shortcuts work with non-Latin keyboard layouts (e.g. Arabic); touchpad gestures follow the fingers; configurable mouse side buttons.
- mpv scripts (Lua / JS) from the `scripts` folder; daily update check (can be turned off).

**Full Arabic interface**: right-to-left, using the Arabeyes technical dictionary, with proper Arabic shaping everywhere. Choose Automatic (system language), English or Arabic in Preferences.

**Not available compared with IINA**: IINA's JavaScript plugins (built on macOS APIs; use mpv scripts instead) and Live Text (Apple Vision).

### Install

**Windows**: run the installer `IINA-RS-x.y.z-windows-x64-setup.exe`. It installs for your account without admin rights (or for all users), adds a Start menu shortcut, offers to add IINA-RS to "Open with", and uninstalls from Windows Settings. Or use the portable build: unzip `IINA-RS-x.y.z-windows-x64.zip` and run `iina-rs.exe` (`libmpv-2.dll` is included). For YouTube links, put `yt-dlp.exe` next to it or on your PATH.

**Linux**:
```bash
sudo apt install ./iina-rs_0.5.1_amd64.deb
```
or from the archive:
```bash
tar xf IINA-RS-0.5.1-linux-x86_64.tar.gz && ./IINA-RS-0.5.1-linux-x86_64/install.sh
```
Requires libmpv: `libmpv2` (Debian/Ubuntu), `mpv-libs` (Fedora) or `mpv` (Arch). The binary needs only glibc ≥ 2.31; libmpv, OpenGL and X11 / Wayland are loaded at run time.

**macOS (Apple Silicon)**: open `IINA-RS-x.y.z-macos-arm64.dmg` and drag the app to Applications. It isn't notarized by Apple, so right-click it and choose Open the first time.

### Shortcuts

| Key | Action |
|---|---|
| Space | Play / pause |
| ← / → | Seek ±5 s |
| ↑ / ↓ | Volume ±5 |
| Ctrl+O / Ctrl+U | Open file / URL |
| Ctrl+Shift+P | Playlist |
| Ctrl+Shift+V / A / S | Video / audio / subtitle settings |
| f, F11, Enter / Esc | Fullscreen / exit |
| Ctrl+Alt+P / T / M | Picture-in-Picture / float on top / music mode |
| Ctrl+S | Screenshot |
| Z / X | Subtitle delay |
| ` and 1–8 | Window size (as in PotPlayer): minimum, 0.5×–2×, maximum, maximize, fullscreen, fit aspect |
| Ctrl+I / Ctrl+J / Ctrl+H | Inspector / jump to time / history |
| Ctrl+, | Preferences |
| Ctrl+Shift+D | Find subtitles online |

Change shortcuts in Preferences → Controls → "Edit Key Bindings…"; they are saved to `input.conf` (mpv / IINA format) in the settings folder: Windows `%APPDATA%\iina-rs\`, Linux `~/.config/iina-rs/`. `mpv.conf` is read from the same folder (mpv's global config is not).

### Build

```bash
cargo build --release
# Linux binary from Windows or macOS:
pip install ziglang && cargo install cargo-zigbuild
cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.31
# packages:
python scripts/package.py windows target/release/iina-rs.exe path/to/libmpv-2.dll dist
python scripts/package.py linux target/x86_64-unknown-linux-gnu/release/iina-rs dist
```
The GitHub Actions workflow in `.github/workflows/build.yml` builds all three platforms and publishes a release when a `v*` tag is pushed, using `docs/releases/<tag>.md` as its description.

### License

GPL-3.0-or-later, like IINA. The key-binding files and app icon come from IINA (© IINA contributors); playback is provided by mpv.
