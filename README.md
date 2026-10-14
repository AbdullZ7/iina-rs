# IINA-RS

مشغّل وسائط لـ **Windows** و **Linux** بروح [IINA](https://github.com/iina/iina)، مكتوب بـ **Rust** ومبني على **libmpv** ([mpv.io](https://mpv.io)).

A Windows & Linux media player in the spirit of IINA, written in Rust on top of libmpv.

## لماذا ليس "تحويل" مباشر؟ / Why a rewrite?

تحليل `IINA.v1.5.0.dmg`:

| المكوّن | المحتوى |
|---|---|
| `MacOS/IINA` | Mach-O universal (x86_64 + arm64)، مكتوب بـ Swift + AppKit (واجهة macOS فقط) |
| `Frameworks/` | `libmpv.2.dylib` + FFmpeg (`libavcodec.63` …) + libass, libplacebo, dav1d … + Sparkle (تحديثات) |
| `MacOS/youtube-dl` | yt-dlp مدمج |
| `Resources/config/*.conf` | اختصارات لوحة المفاتيح (صيغة mpv) |
| `Resources/plugins/` | إضافات JS (opensub, ytdl, userscript) |

The binaries are Mach-O and the UI is AppKit, so they cannot run on Windows or Linux.
The playback core (mpv) is cross-platform, though. IINA-RS keeps that same core,
reuses IINA's key-binding files and icon (GPLv3), and rebuilds the IINA UI in Rust.

## الميزات / Features

- تشغيل بـ libmpv عبر OpenGL render API (فك تشفير بالعتاد: d3d11va / vaapi / nvdec)
- شريط تحكّم عائم بأسلوب IINA يختفي تلقائياً، وشريط تقدّم مع علامات الفصول ومعاينة الوقت
- شريط جانبي: قائمة التشغيل، الفصول، إعدادات الفيديو/الصوت/الترجمة (المسارات، السرعة، التأخير، نسبة العرض، التدوير، …)
- اختصارات IINA الأصلية (`iina-default-input.conf`)، مع تحويل ⌘ إلى Ctrl
- Picture-in-Picture، Music mode، Float on top، ملء الشاشة بالنقر المزدوج
- السحب والإفلات (فيديو أو ترجمة)، فتح روابط (يوتيوب عبر yt-dlp)
- استئناف التشغيل من آخر موضع، قائمة الملفات الأخيرة
- عجلة الفأرة: عمودياً = الصوت، أفقياً = التقديم
- خيارات mpv من سطر الأوامر: `iina-rs --start=30 --volume=50 file.mkv`
- معاينة مصغّرة (thumbnail) عند تمرير الماوس على شريط الوقت، تُولَّد في الخلفية بنسخة mpv ثانية
- إظهار الوقت المتبقي (انقر على المدة)، ومؤشر التخزين المؤقت وحلقة A-B على الشريط
- إضافة باقي ملفات المجلد تلقائياً لقائمة التشغيل بترتيب طبيعي (ep2 قبل ep10)
- معادل صورة (سطوع/تباين/تشبع/جاما/تدرج اللون) وقلب/عكس الصورة، ومعادل صوت 10 نطاقات مع إعدادات جاهزة
- ترجمة ثانوية، لون الترجمة، نقل عناصر قائمة التشغيل
- نافذة Inspector (Ctrl+I) بمعلومات الملف والمسارات والإطارات المُسقطة
- الانتقال إلى وقت محدد (Ctrl+J) — يقبل `1:23:45` أو `50%`
- نافذة التفضيلات (Ctrl+,)
- البحث عن ترجمات أونلاين وتحميلها (Ctrl+Shift+D) من OpenSubtitles.com — بالبصمة (hash) والاسم؛ يحتاج مفتاح API مجاني يُوضع في التفضيلات
- سجل التشغيل (Ctrl+H) مع نسبة التقدم والبحث
- نسخة واحدة: فتح ملف والبرنامج شغّال يفتحه في نفس النافذة
- قص الصورة (crop) بنسب جاهزة
- ويندوز: إضافة البرنامج لقائمة "فتح باستخدام" من التفضيلات
- سكربتات mpv (Lua/JS) من مجلد `scripts` داخل مجلد الإعدادات

### غير متوفر مقارنةً بـ IINA
- إضافات IINA بـ JavaScript (واجهتها مبنية على macOS) — البديل: سكربتات mpv.
- Live Text (يعتمد على Apple Vision).

## التثبيت / Install

### Windows
فكّ ضغط `IINA-RS-x.y.z-windows-x64.zip` وشغّل `iina-rs.exe` (يأتي معه `libmpv-2.dll`).
لروابط يوتيوب ضع `yt-dlp.exe` في نفس المجلد أو في PATH.

### Linux
```bash
sudo apt install ./iina-rs_0.1.0_amd64.deb
```
أو من الأرشيف:
```bash
tar xf IINA-RS-0.1.0-linux-x86_64.tar.gz && ./IINA-RS-0.1.0-linux-x86_64/install.sh
```
يتطلب libmpv: `libmpv2` (Debian/Ubuntu)، `mpv-libs` (Fedora)، `mpv` (Arch).
الملف التنفيذي لا يعتمد إلا على glibc ≥ 2.31؛ و libmpv و OpenGL و X11/Wayland تُحمَّل وقت التشغيل.

## الاختصارات / Shortcuts (من IINA)

| المفتاح | الوظيفة |
|---|---|
| Space | تشغيل / إيقاف |
| ← / → | تقديم ±5 ثوانٍ |
| ↑ / ↓ | الصوت ±5 |
| Ctrl+O / Ctrl+U | فتح ملف / رابط |
| Ctrl+Shift+P | قائمة التشغيل |
| Ctrl+Shift+V / A / S | إعدادات الفيديو / الصوت / الترجمة |
| f, F11, Ctrl+Alt+F, Enter / Esc | ملء الشاشة / خروج |
| Ctrl+Alt+P | Picture-in-Picture |
| Ctrl+Alt+T | فوق كل النوافذ |
| Ctrl+Alt+M | Music mode |
| Ctrl+S | لقطة شاشة |
| Ctrl+Alt+S / Ctrl+Alt+A | تبديل مسار الترجمة / الصوت |
| Ctrl+L | تكرار A-B |
| Ctrl+[ / Ctrl+] / Ctrl+\ | السرعة ½× / 2× / 1× |
| Z / X | تأخير الترجمة ∓0.5s |
| Ctrl+I | Inspector |
| Ctrl+J | الانتقال إلى وقت |
| Ctrl+, | التفضيلات |
| Ctrl+H | سجل التشغيل |
| Ctrl+Shift+D | البحث عن ترجمات أونلاين |

لتخصيص الاختصارات أنشئ `input.conf` (صيغة mpv/IINA) في مجلد الإعدادات:
Windows `%APPDATA%\iina-rs\` — Linux `~/.config/iina-rs/`. ويُقرأ `mpv.conf` من نفس المجلد.

## البناء / Build

```bash
cargo build --release
# Linux binary from Windows/macOS:
pip install ziglang && cargo install cargo-zigbuild
cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.31
# packages:
python scripts/package.py windows target/release/iina-rs.exe path/to/libmpv-2.dll dist
python scripts/package.py linux target/x86_64-unknown-linux-gnu/release/iina-rs dist
```
The GitHub Actions workflow in `.github/workflows/build.yml` builds both platforms and
publishes a release when you push a `v*` tag.

## الترخيص / License

GPL-3.0-or-later, same as IINA. The key-binding files and app icon come from IINA
(© IINA contributors). Video playback is provided by mpv.
