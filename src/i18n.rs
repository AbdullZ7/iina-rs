//! Interface languages (English, Arabic) and Arabic text display.
//!
//! egui neither joins Arabic letters nor reorders right-to-left text, so text
//! is converted for display: letters are replaced by their contextual
//! presentation forms (ar-reshaper) and runs are put in visual order with the
//! Unicode bidi algorithm (unicode-bidi), mirroring brackets in RTL runs.
//! Strings are kept in logical order everywhere and converted only when drawn
//! (`t`, `tf`, `v`). mpv's OSD and subtitles go through libass, which shapes
//! Arabic itself, so they receive logical text (`tl`, `tlf`).
//!
//! Arabic terminology follows the conventions of the Arabeyes (عرب آيز)
//! free-software localization project used across KDE/GNOME.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    English = 0,
    Arabic = 1,
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_language(l: Lang) {
    LANG.store(l as u8, Ordering::Relaxed);
}

pub fn language() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 { Lang::Arabic } else { Lang::English }
}

pub fn is_rtl() -> bool {
    language() == Lang::Arabic
}

/// Resolves the "language" setting ("auto", "en", "ar").
pub fn resolve(setting: &str) -> Lang {
    match setting {
        "ar" => Lang::Arabic,
        "en" => Lang::English,
        _ => {
            let sys = sys_locale::get_locale().unwrap_or_default().to_lowercase();
            if sys.starts_with("ar") { Lang::Arabic } else { Lang::English }
        }
    }
}

const AR: &[(&str, &str)] = &[
    // Controller
    ("Mute", "اكتم"),
    ("Volume {}", "حجم الصوت {}"),
    ("Previous", "السابق"),
    ("Play", "شغّل"),
    ("Pause", "توقف مؤقتًا"),
    ("Next", "التالي"),
    ("Fullscreen", "ملء الشاشة"),
    ("Picture-in-Picture", "صورة داخل صورة"),
    ("Playlist", "قائمة التشغيل"),
    ("Subtitles", "الترجمات"),
    ("Video & Audio settings", "إعدادات المرئيات والسمعيات"),
    ("Click to toggle remaining time", "انقر للتبديل إلى الوقت الباقي"),
    // Welcome
    ("Version {}", "الإصدار {}"),
    ("Open File…", "افتح ملفًا…"),
    ("Open URL…", "افتح مسارًا…"),
    ("Recent", "المفتوحة مؤخراً"),
    ("Drop a video here, or open a file.", "اسحب ملفًا مرئيًا إلى هنا، أو افتح ملفًا."),
    ("Clear recent", "امحُ القائمة"),
    // Sidebar
    ("None", "بلا"),
    ("(external)", "(خارجي)"),
    ("Reset", "صفِّر"),
    ("Flat", "مسطح"),
    ("Bass Boost", "تعزيز الجهير"),
    ("Treble Boost", "تعزيز الحادّ"),
    ("Vocal", "صوتي"),
    ("Rock", "روك"),
    ("Loudness", "جهارة"),
    ("Chapters", "الفصول"),
    ("Video", "المرئيات"),
    ("Audio", "السمعيات"),
    ("No chapters", "لا توجد فصول"),
    ("Chapter {}", "الفصل {}"),
    ("Subtitle track", "مسار الترجمة"),
    ("Load external subtitle…", "حمّل ترجمة خارجية…"),
    ("Secondary subtitle", "الترجمة الثانوية"),
    ("Show subtitles", "إظهار الترجمة"),
    ("Subtitle delay", "تأخير الترجمة"),
    ("Text size", "حجم النص"),
    ("Position", "الموضع"),
    ("Text color", "لون النص"),
    ("White", "أبيض"),
    ("Yellow", "أصفر"),
    ("Cyan", "سماوي"),
    ("Green", "أخضر"),
    ("Add files…", "أضِف ملفات…"),
    ("Shuffle", "اخلط"),
    ("Loop", "تكرار"),
    ("Move Up", "حرّك لأعلى"),
    ("Move Down", "حرّك لأسفل"),
    ("Remove", "أزِل"),
    ("Show in Folder", "أظهِر في المجلد"),
    ("Video track", "المسار المرئي"),
    ("Speed", "السرعة"),
    ("Aspect ratio", "النسبة الباعيّة"),
    ("Default", "مبدئي"),
    ("Crop", "الاقتصاص"),
    ("Rotation", "الدوران"),
    ("Flip", "قلب عمودي"),
    ("Mirror", "مرآة"),
    ("Deinterlace", "إزالة التشابك"),
    ("Video equalizer", "معادِل المرئيات"),
    ("Brightness", "السطوع"),
    ("Contrast", "التباين"),
    ("Saturation", "التشبع"),
    ("Gamma", "جاما"),
    ("Hue", "تدرج اللون"),
    ("Reset equalizer", "صفِّر المعادِل"),
    ("Hardware decoding", "فك الترميز بالعتاد"),
    ("Decoder: {}", "مفكك الترميز: {}"),
    ("software", "برمجية"),
    ("Take screenshot", "التقط لقطة شاشة"),
    ("Audio track", "المسار السمعي"),
    ("Load external audio…", "حمّل مسارًا سمعيًا خارجيًا…"),
    ("Volume", "حجم الصوت"),
    ("Audio delay", "التأخير السمعي"),
    ("Equalizer", "المعادِل"),
    // Context menu
    ("Jump to…", "اقفز إلى…"),
    ("Find Online Subtitles…", "ابحث عن ترجمات على الإنترنت…"),
    ("Take Screenshot", "التقط لقطة شاشة"),
    ("Stop", "أوقِف"),
    ("Video Settings", "إعدادات المرئيات"),
    ("Audio Settings", "إعدادات السمعيات"),
    ("Subtitle Settings", "إعدادات الترجمة"),
    ("Exit Fullscreen", "اخرج من ملء الشاشة"),
    ("Enter Fullscreen", "ادخل ملء الشاشة"),
    ("✓ Float on Top", "✓ فوق كل النوافذ"),
    ("Float on Top", "فوق كل النوافذ"),
    ("✓ Picture-in-Picture", "✓ صورة داخل صورة"),
    ("✓ Music Mode", "✓ وضع الموسيقى"),
    ("Music Mode", "وضع الموسيقى"),
    ("Inspector", "المفتّش"),
    ("Playback History", "تاريخ التشغيل"),
    ("Preferences…", "التفضيلات…"),
    ("About IINA-RS", "عن IINA-RS"),
    ("Quit", "غادر"),
    // Dialogs
    ("Open URL", "فتح مسار"),
    ("Enter a media URL (http, https, YouTube via yt-dlp, …):", "أدخِل مسار وسائط (http أو https أو يوتيوب عبر yt-dlp…):"),
    ("Open", "افتح"),
    ("Cancel", "ألغِ"),
    ("Jump to", "القفز إلى"),
    ("Time (hh:mm:ss) or percentage — duration {}", "الوقت (ساعة:دقيقة:ثانية) أو نسبة مئوية — المدة {}"),
    ("1:23:45  or  50%", "1:23:45 أو 50%"),
    ("Jump", "اقفز"),
    // Inspector
    ("Nothing is playing.", "لا شيء قيد التشغيل."),
    ("General", "عام"),
    ("Path", "المسار"),
    ("Format", "النسق"),
    ("Size", "الحجم"),
    ("Duration", "المدة"),
    ("Codec", "المرماز"),
    ("Resolution", "الميز"),
    ("Display", "العرض"),
    ("Pixel format", "نسق البكسل"),
    ("Colors", "الألوان"),
    ("Frame rate", "معدل الإطارات"),
    ("Bitrate", "معدل البت"),
    ("Hardware decoder", "مفكك العتاد"),
    ("Sample rate", "معدل الاعتيان"),
    ("Channels", "القنوات"),
    ("Playback", "التشغيل"),
    ("Dropped (VO / decoder)", "الإطارات المُسقطة (العرض / المفكك)"),
    ("A/V sync", "تزامن الصوت والصورة"),
    ("Cache", "التخزين المؤقت"),
    // Preferences
    ("Preferences", "التفضيلات"),
    ("Language", "اللغة"),
    ("Automatic", "آلي"),
    ("Resume last playback position", "استئناف التشغيل من آخر موضع"),
    ("Add files in the same folder to the playlist", "إضافة ملفات المجلد نفسه إلى قائمة التشغيل"),
    ("Resize window to video size when opening a file", "ملاءمة حجم النافذة للمرئي عند فتح ملف"),
    ("Open files in the running window (single instance)", "فتح الملفات في النافذة المفتوحة (نسخة واحدة)"),
    ("UI", "الواجهة"),
    ("Show thumbnail preview on the seek bar", "إظهار معاينة مصغّرة على شريط السعي"),
    ("Show remaining time", "إظهار الوقت الباقي"),
    ("Single click on video toggles pause", "نقرة واحدة على المرئي توقفه مؤقتًا"),
    ("Hide controller after", "إخفاء المتحكم بعد"),
    ("Video / Audio", "المرئيات / السمعيات"),
    ("Enable hardware decoding", "تفعيل فك الترميز بالعتاد"),
    ("Initial volume", "حجم الصوت الابتدائي"),
    ("Screenshots", "لقطات الشاشة"),
    ("Choose…", "اختر…"),
    ("Online subtitles", "الترجمات على الإنترنت"),
    ("SubDL key", "مفتاح SubDL"),
    ("Get a key", "احصل على مفتاح"),
    ("OpenSubtitles key", "مفتاح OpenSubtitles"),
    ("Languages", "اللغات"),
    ("System", "النظام"),
    ("Add to “Open with”", "أضِف إلى «فتح باستخدام»"),
    ("Registers IINA-RS for video/audio files for your user account", "يسجّل IINA-RS للملفات المرئية والسمعية في حساب المستخدم"),
    ("Default apps…", "التطبيقات المبدئية…"),
    ("Open settings folder", "افتح مجلد الإعدادات"),
    (
        "Settings folder: input.conf (key bindings), mpv.conf (mpv options), scripts/ (mpv Lua/JS scripts).",
        "مجلد الإعدادات: input.conf (اختصارات لوحة المفاتيح)، mpv.conf (خيارات mpv)، scripts/ (سكربتات mpv بلغة Lua/JS).",
    ),
    // About
    ("A Windows & Linux media player in the spirit of IINA,", "مشغّل وسائط لويندوز ولينكس بروح IINA،"),
    ("written in Rust and powered by mpv (mpv.io).", "مكتوب بلغة Rust ومبني على mpv (mpv.io)."),
    ("Arabic translation follows Arabeyes conventions.", "الترجمة العربية وفق مصطلحات عرب آيز."),
    // History
    ("Search…", "بحث…"),
    ("Clear History", "امحُ التاريخ"),
    ("No history yet.", "لا تاريخ بعد."),
    ("Remove from History", "أزِل من التاريخ"),
    ("just now", "الآن"),
    ("{} min ago", "قبل {} دقيقة"),
    ("{} h ago", "قبل {} ساعة"),
    ("yesterday", "أمس"),
    ("{} days ago", "قبل {} يوم"),
    ("{} months ago", "قبل {} شهر"),
    // Online subtitles
    ("Find Online Subtitles", "البحث عن ترجمات على الإنترنت"),
    ("Source", "المصدر"),
    ("A free {} API key is required. Create one, then paste it in Preferences.", "يلزم مفتاح API مجاني من {}. أنشئ مفتاحاً ثم الصقه في التفضيلات."),
    ("Get an API key", "احصل على مفتاح API"),
    ("Open Preferences", "افتح التفضيلات"),
    ("Title", "العنوان"),
    ("Search", "ابحث"),
    ("(no release name)", "(بدون اسم إصدار)"),
    ("Likely synced to this file", "غالباً متزامنة مع هذا الملف"),
    ("Matched by title", "مطابقة بالعنوان"),
    ("By {}", "بواسطة {}"),
    ("Hearing impaired (SDH)", "لضعاف السمع (SDH)"),
    ("Download", "نزِّل"),
    ("Searching {}…", "جارٍ البحث في {}…"),
    ("No subtitles found.", "لم يُعثر على ترجمات."),
    ("{} results from {}", "{} نتيجة من {}"),
    ("Downloading…", "جارٍ التنزيل…"),
    ("Loaded {}", "تم تحميل {}"),
    ("{} rejected the API key. Check it in Preferences.", "رفض {} مفتاح API. افحصه في التفضيلات."),
    ("Network error: {}", "خطأ في الشبكة: {}"),
    ("The archive contains no subtitle file.", "لا يحتوي الملف المضغوط على ترجمة."),
    // On-screen messages
    ("Volume: {}", "حجم الصوت: {}"),
    ("Unmute", "ألغِ الكتم"),
    ("Speed: {}x", "السرعة: {}x"),
    ("Resume", "استئناف"),
    ("Subtitle Delay: {}s", "تأخير الترجمة: {} ث"),
    ("Audio Delay: {}s", "التأخير السمعي: {} ث"),
    ("Subtitle: {}", "الترجمة: {}"),
    ("Secondary Subtitle: {}", "الترجمة الثانوية: {}"),
    ("Audio: {}", "المسار السمعي: {}"),
    ("Video: {}", "المسار المرئي: {}"),
    ("Subtitle Scale: {}x", "حجم الترجمة: {}x"),
    ("Subtitle Position: {}", "موضع الترجمة: {}"),
    ("Subtitles Hidden", "الترجمة مخفية"),
    ("Subtitles Visible", "الترجمة ظاهرة"),
    ("Loop: File", "التكرار: الملف"),
    ("Loop: Playlist", "التكرار: قائمة التشغيل"),
    ("Loop: Off", "التكرار: مغلق"),
    ("A-B Loop: A = {}", "تكرار A-B: البداية {}"),
    ("A-B Loop: {} – {}", "تكرار A-B: {} – {}"),
    ("A-B Loop: Off", "تكرار A-B: مغلق"),
    ("Aspect Ratio: {}", "النسبة الباعيّة: {}"),
    ("Rotation: {}°", "الدوران: {}°"),
    ("Deinterlace: {}", "إزالة التشابك: {}"),
    ("Hardware Decoding: {}", "فك الترميز بالعتاد: {}"),
    ("On", "يعمل"),
    ("Off", "مغلق"),
    ("Screenshot Captured", "التُقطت لقطة الشاشة"),
    ("Chapter: {}", "الفصل: {}"),
    ("HDR output on HDR displays (restart required)", "خَرْج HDR على الشاشات الداعمة (يتطلب إعادة التشغيل)"),
    ("Super resolution: sharpens and cleans up enlarged video", "الميز الفائق: يوضّح المرئيات المكبَّرة وينظّفها"),
    ("RTX Video HDR: shows SDR video in HDR", "RTX Video HDR: يعرض المرئيات العادية بنطاق HDR"),
    ("Super resolution is active now: ×{}", "الميز الفائق يعمل الآن: ×{}"),
    ("Super resolution isn't active now: the video isn't enlarged", "الميز الفائق لا يعمل الآن: المرئيات غير مكبَّرة"),
    ("RTX Video HDR is active now", "يعمل RTX Video HDR الآن"),
    ("Takes effect after a restart", "يسري بعد إعادة التشغيل"),
    ("IINA-RS is running on: {}", "يعمل IINA-RS الآن على: {}"),
    ("RTX Video needs the NVIDIA graphics card", "يتطلب RTX Video بطاقة رسوميات NVIDIA"),
    ("Use the NVIDIA graphics card", "استعمل بطاقة رسوميات NVIDIA"),
    ("RTX Video HDR needs an HDR display", "يتطلب RTX Video HDR شاشة HDR"),
    ("If the picture doesn't change, turn on RTX Video Enhancement in the NVIDIA Control Panel", "إن لم تتغير الصورة ففعّل خيار RTX Video Enhancement في لوحة تحكم NVIDIA"),
    ("Output", "الخَرْج"),
    ("HDR peak brightness", "ذروة سطوع HDR"),
    ("nits (0 = automatic)", "نت (0 = آلي)"),
    // Subtitle preferences
    ("Preferred languages", "اللغات المفضّلة"),
    ("Font", "الخط"),
    ("Default font", "الخط المبدئي"),
    ("Search fonts…", "بحث في الخطوط…"),
    ("Loading fonts…", "جارٍ تحميل الخطوط…"),
    ("Bold", "ثخين"),
    ("Size", "الحجم"),
    ("Text", "النص"),
    ("Outline", "الحد"),
    ("Shadow", "الظل"),
    ("Letter spacing", "مباعدة الحروف"),
    ("Style", "النمط"),
    ("Outline & shadow", "حد وظل"),
    ("Opaque box", "صندوق معتم"),
    ("Background box", "صندوق خلفية"),
    ("Box color", "لون الصندوق"),
    ("Vertical position", "الموضع العمودي"),
    ("Bottom margin", "الهامش السفلي"),
    ("Secondary subtitle position", "موضع الترجمة الثانوية"),
    ("Encoding", "الترميز"),
    ("Auto-detect", "كشف آلي"),
    ("Arabic (Windows-1256)", "العربية (Windows-1256)"),
    ("ASS/SSA styling", "تنسيق ASS/SSA"),
    ("As authored", "كما في الملف"),
    ("Use my style", "استخدام نمطي"),
    ("Remove styling", "إزالة التنسيق"),
    ("Load external subtitles", "تحميل الترجمات الخارجية"),
    ("Exact file name", "نفس اسم الملف"),
    ("Containing the file name", "تحتوي اسم الملف"),
    ("Any in the folder", "أي ترجمة في المجلد"),
    ("Show text subtitles in the black bars", "إظهار الترجمة النصية في الأشرطة السوداء"),
    ("Remove hearing-impaired text", "إزالة نصوص ضعاف السمع"),
    ("Sync steps (s)", "خطوات المزامنة (ث)"),
    ("Open fonts folder", "افتح مجلد الخطوط"),
    ("Shortcuts: Z/X, Ctrl+Shift+Z/X, Ctrl+Alt+Shift+Z/X", "المختصرات: Z/X و Ctrl+Shift+Z/X و Ctrl+Alt+Shift+Z/X"),
    ("Video & Audio", "المرئيات والسمعيات"),
    ("Network", "الشبكة"),
    ("Controls", "التحكم"),
    ("Advanced", "متقدم"),
    ("Pause when a file opens", "توقف مؤقت عند فتح ملف"),
    ("Enter fullscreen when a file opens", "ملء الشاشة عند فتح ملف"),
    ("Keep the display awake during playback", "إبقاء الشاشة مستيقظة أثناء التشغيل"),
    ("Window", "النافذة"),
    ("Window scale", "مقياس النافذة"),
    ("On-screen controller", "المتحكم على الشاشة"),
    ("Hide the controller automatically", "إخفاء المتحكم آليًا"),
    ("Thumbnail width", "عرض المعاينة المصغّرة"),
    ("pt (0 = automatic)", "نقطة (0 = آلي)"),
    ("On-screen messages", "الرسائل على الشاشة"),
    ("Show on-screen messages", "إظهار الرسائل على الشاشة"),
    ("File name template", "قالب اسم الملف"),
    ("Include subtitles", "تضمين الترجمة"),
    ("Automatic (safe)", "آلي (آمن)"),
    ("Automatic (all)", "آلي (الكل)"),
    ("Automatic, copy back", "آلي مع النسخ"),
    ("Preferred audio languages", "لغات السمعيات المفضّلة"),
    ("Output device", "جهاز الخَرْج"),
    ("Remember the last volume", "تذكّر آخر حجم للصوت"),
    ("Maximum volume", "الحجم الأقصى للصوت"),
    ("Pass through to an external decoder (S/PDIF / HDMI receiver):", "تمرير السمعيات لمفكك خارجي (S/PDIF / مستقبل HDMI):"),
    ("Track", "المسار"),
    ("Album", "الألبوم"),
    ("Gapless audio", "سمعيات بلا فواصل"),
    ("Enable cache", "تفعيل التخزين المؤقت"),
    ("Cache size", "حجم التخزين المؤقت"),
    ("Read ahead", "القراءة المسبقة"),
    ("Connection", "الاتصال"),
    ("HTTP proxy", "وسيط HTTP"),
    ("User agent", "وكيل المستخدم"),
    ("Play YouTube and other sites with yt-dlp", "تشغيل يوتيوب والمواقع الأخرى عبر yt-dlp"),
    ("yt-dlp path", "مسار yt-dlp"),
    ("yt-dlp options", "خيارات yt-dlp"),
    ("Mouse", "الفأرة"),
    ("Play / pause", "تشغيل / توقف مؤقت"),
    ("Hide controller", "إخفاء المتحكم"),
    ("Context menu", "القائمة السياقية"),
    ("Single click", "نقرة واحدة"),
    ("Double click", "نقرتان"),
    ("Right click", "النقر الأيمن"),
    ("Middle click", "النقر الأوسط"),
    ("Seek", "السعي"),
    ("Vertical scroll", "التمرير العمودي"),
    ("Horizontal scroll", "التمرير الأفقي"),
    ("Controller buttons", "أزرار المتحكم"),
    ("⏪ ⏩ buttons", "زرّا ⏪ ⏩"),
    ("Change speed", "تغيير السرعة"),
    ("Previous / next file", "الملف السابق / التالي"),
    ("Seek step", "خطوة السعي"),
    ("Key bindings: input.conf in the settings folder (IINA / mpv format).", "ارتباطات المفاتيح: ملف input.conf في مجلد الإعدادات (نسق IINA / mpv)."),
    ("mpv options", "خيارات mpv"),
    ("One option per line, e.g. deband=yes. Applied when IINA-RS starts.", "خيار في كل سطر، مثل deband=yes. تُطبّق عند تشغيل IINA-RS."),
    ("Slower", "أبطئ"),
    ("Faster", "أسرِع"),
    ("Rewind", "إلى الوراء"),
    ("Forward", "إلى الأمام"),
    ("Added {} files to playlist", "أُضيف {} ملف لقائمة التشغيل"),
    ("Crop: {}", "الاقتصاص: {}"),
    ("Subtitle Delay: {}", "تأخير الترجمة: {}"),
    ("Audio Delay: {}", "التأخير السمعي: {}"),
    ("No Delay", "بدون تأخير"),
    ("{}s Later", "متأخرة {} ث"),
    ("{}s Earlier", "متقدّمة {} ث"),
    ("Enable file looping", "فعّل تكرار الملف"),
    ("Enable playlist looping", "فعّل تكرار قائمة التشغيل"),
    ("Disable loop", "عطّل التكرار"),
    ("Move “{}” to the Recycle Bin?", "تحريك «{}» إلى سلة المحذوفات؟"),
    ("Italic", "مائل"),
    ("Blur", "الضبابية"),
    ("Side margin", "الهامش الجانبي"),
    ("Scale subtitles with the window", "تحجيم الترجمة مع النافذة"),
    ("Also look for subtitles in subfolders (Subs/, Subtitles/…)", "البحث عن الترجمات في المجلدات الفرعية أيضاً (Subs/ و Subtitles/…)"),
    ("Move to Recycle Bin…", "حرّك إلى سلة المحذوفات…"),
    ("Save Playlist…", "احفظ قائمة التشغيل…"),
    ("Key Bindings", "ارتباطات المفاتيح"),
    ("Key Bindings…", "ارتباطات المفاتيح…"),
    ("Edit Key Bindings…", "حرِّر ارتباطات المفاتيح…"),
    ("Keyboard", "لوحة المفاتيح"),
    ("Add", "أضِف"),
    ("Restore defaults", "استعد المبدئيات"),
    ("Save", "احفظ"),
    ("Click a key to change it, then press the new key. Commands use mpv's input.conf syntax; ✓ app marks IINA-RS commands.", "انقر على المفتاح لتغييره ثم اضغط المفتاح الجديد. الأوامر بصياغة input.conf الخاصة بـ mpv، وعلامة «تطبيق» تعني أمرًا خاصًا بـ IINA-RS."),
    ("Press a key…", "اضغط مفتاحاً…"),
    ("(none)", "(بلا)"),
    ("app", "تطبيق"),
    ("Filters", "المرشّحات"),
    ("Filters…", "المرشّحات…"),
    ("Video filters", "مرشّحات مرئية"),
    ("Audio filters", "مرشّحات سمعية"),
    ("Saved filters", "المرشّحات المحفوظة"),
    ("audio", "سمعي"),
    ("video", "مرئي"),
    ("Delete", "احذف"),
    ("New filter", "مرشّح جديد"),
    ("Name", "الاسم"),
    ("Filter", "المرشّح"),
    ("mpv filter syntax, e.g. lavfi=[unsharp], hflip, scale=1280:-2.", "بصياغة مرشّحات mpv، مثل lavfi=[unsharp] أو hflip أو scale=1280:-2."),
    ("Add and Save", "أضِف واحفظ"),
    ("Logs", "السجلات"),
    ("All", "الكل"),
    ("Warnings", "الإنذارات"),
    ("Errors", "الأخطاء"),
    ("Copy", "انسخ"),
    ("Save…", "احفظ…"),
    ("{} lines", "{} سطر"),
    ("Copied to clipboard", "نُسخت إلى الحافظة"),
    ("IINA-RS {} is available.", "الإصدار {} من IINA-RS متوفر."),
    ("Check for Updates", "افحص التحديثات"),
    ("Checking…", "جارٍ الفحص…"),
    ("You have the latest version.", "لديك أحدث إصدار."),
    ("Couldn't check for updates.", "تعذّر فحص التحديثات."),
    ("Exit Music Mode", "اخرج من وضع الموسيقى"),
    ("Show Playlist", "أظهِر قائمة التشغيل"),
    ("Hide Playlist", "أخفِ قائمة التشغيل"),
    ("Pause when minimized, resume when restored", "توقف مؤقت عند التصغير، واستئناف عند الاستعادة"),
    ("Switch to music mode for audio files", "التحوّل إلى وضع الموسيقى للملفات السمعية"),
    ("Black out other displays in fullscreen", "تعتيم الشاشات الأخرى في وضع ملء الشاشة"),
    ("Floating", "عائمة"),
    ("Bar at the bottom", "شريط في الأسفل"),
    ("Bar at the top", "شريط في الأعلى"),
    ("Copy to the clipboard", "النسخ إلى الحافظة"),
    ("Show a preview after taking a screenshot", "إظهار معاينة بعد التقاط لقطة شاشة"),
    ("Updates", "التحديثات"),
    ("Check for updates automatically (once a day, GitHub)", "فحص التحديثات آليًا (مرة يوميًا، من GitHub)"),
    ("Screenshot copied to clipboard", "نُسخت لقطة الشاشة إلى الحافظة"),
    ("Key bindings saved", "حُفظت ارتباطات المفاتيح"),
    ("Paused", "توقف مؤقت"),
    ("Stopped", "وقوف"),
    ("Muted", "كتم"),
    ("Unmuted", "إلغاء الكتم"),
    ("Choose a font…", "اختر خطًّا…"),
    ("Font style", "نمط الخط"),
    ("Regular", "منتظِم"),
    ("Bold Italic", "ثخين مائل"),
    ("Sample", "عيّنة"),
    ("The subtitle on the video changes as you choose.", "تتغيّر الترجمة على المرئي مع كل اختيار."),
    ("OK", "موافق"),
    ("Side buttons (4 / 5)", "الزرّان الجانبيان (4 / 5)"),
    ("Side button step", "خطوة الزرّين الجانبيين"),
    ("Window size: {}%", "حجم النافذة: {}%"),
    ("Scale subtitles with the window (not the video)", "تحجيم الترجمة مع النافذة (لا مع المرئي)"),
    ("Lift subtitles above the controller while it is shown", "رفع الترجمة فوق المتحكم عند ظهوره"),
    ("Normalize audio (boost quiet parts)", "تسوية الصوت (رفع المقاطع الخافتة)"),
    ("Audio normalization: On", "تسوية الصوت: يعمل"),
    ("Audio normalization: Off", "تسوية الصوت: مغلق"),
    ("Touchpad: follow the fingers (two fingers up = louder)", "لوحة اللمس: اتباع الأصابع (إصبعان للأعلى = صوت أعلى)"),
    ("Window size: Minimum", "حجم النافذة: الأصغر"),
    ("Window size: Maximum", "حجم النافذة: الأكبر"),
    ("Video can't be shown on this device", "تعذّر عرض المرئيات على هذا الجهاز"),
    ("The graphics card's driver doesn't provide OpenGL 3.0 or newer, which mpv needs.", "تعريف بطاقة الرسوميات لا يوفّر OpenGL 3.0 أو أحدث، ويحتاجه mpv."),
    ("Update the graphics card driver from its maker's site (Intel, AMD or NVIDIA),", "حدّث تعريف بطاقة الرسوميات من موقع الشركة المصنّعة (Intel أو AMD أو NVIDIA)،"),
    ("then open IINA-RS again.", "ثم افتح IINA-RS من جديد."),
    // App messages
    (
        "Registered. Pick IINA-RS under “Open with”, or set it as default in Default apps.",
        "تم التسجيل. اختر IINA-RS من «فتح باستخدام» أو اجعله الافتراضي من التطبيقات الافتراضية.",
    ),
    ("Removed IINA-RS from “Open with”.", "أُزيل IINA-RS من «فتح باستخدام»."),
    ("Failed: {}", "فشل: {}"),
    ("Not available on this platform yet", "غير متاح على هذا النظام بعد"),
];

fn table() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| AR.iter().copied().collect())
}

/// Translated text in logical order (for mpv/libass, window titles, storage).
pub fn tl(key: &str) -> String {
    match language() {
        Lang::Arabic => table().get(key).copied().unwrap_or(key).to_string(),
        Lang::English => key.to_string(),
    }
}

/// Translated, with `{}` placeholders filled in order (logical order).
pub fn tlf(key: &str, args: &[&dyn Display]) -> String {
    let mut out = String::new();
    let mut args = args.iter();
    let mut rest = tl(key);
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        if let Some(a) = args.next() {
            out.push_str(&a.to_string());
        }
        rest = rest[i + 2..].to_string();
    }
    out.push_str(&rest);
    out
}

/// Translated text ready to draw with egui.
pub fn t(key: &str) -> String {
    let s = tl(key);
    if is_rtl() { visual(&s, Some(true)) } else { s }
}

/// Translated + formatted text ready to draw with egui.
pub fn tf(key: &str, args: &[&dyn Display]) -> String {
    let s = tlf(key, args);
    if is_rtl() { visual(&s, Some(true)) } else { visual(&s, None) }
}

/// Dynamic text (file names, titles, statuses) ready to draw with egui.
pub fn v(text: &str) -> String {
    visual(text, if is_rtl() { None } else { Some(false) })
}

fn has_rtl(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF))
}

fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        _ => c,
    }
}

fn reshaper() -> &'static ar_reshaper::ArabicReshaper {
    static R: OnceLock<ar_reshaper::ArabicReshaper> = OnceLock::new();
    R.get_or_init(|| {
        let mut cfg = ar_reshaper::ReshaperConfig::default();
        // The "Allah" ligature glyph (U+FDF2) is missing from many UI fonts.
        cfg.update_ligature(ar_reshaper::prelude::LigatureNames::ARABIC_LIGATURE_ALLAH, false);
        cfg.to_reshaper()
    })
}

/// Converts logical text to the visual form egui can draw. `rtl`: paragraph
/// direction (None = from the first strong character).
pub fn visual(text: &str, rtl: Option<bool>) -> String {
    if !has_rtl(text) && rtl != Some(true) {
        return text.to_string();
    }
    static CACHE: OnceLock<Mutex<HashMap<(String, Option<bool>), String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap().get(&(text.to_string(), rtl)) {
        return hit.clone();
    }
    let out = text.split('\n').map(|line| visual_line(line, rtl)).collect::<Vec<_>>().join("\n");
    let mut c = cache.lock().unwrap();
    if c.len() > 4000 {
        c.clear();
    }
    c.insert((text.to_string(), rtl), out.clone());
    out
}

fn visual_line(line: &str, rtl: Option<bool>) -> String {
    use unicode_bidi::{BidiInfo, Level};
    if line.is_empty() {
        return String::new();
    }
    let shaped = if has_rtl(line) { reshaper().reshape(line) } else { line.to_string() };
    let base = rtl.map(|r| if r { Level::rtl() } else { Level::ltr() });
    let bidi = BidiInfo::new(&shaped, base);
    let mut out = String::with_capacity(shaped.len());
    for para in &bidi.paragraphs {
        let (levels, runs) = bidi.visual_runs(para, para.range.clone());
        for run in runs {
            let s = &shaped[run.clone()];
            if levels[run.start].is_rtl() {
                out.extend(s.chars().rev().map(mirror));
            } else {
                out.push_str(s);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_text_is_untouched() {
        assert_eq!(visual("Open File…", None), "Open File…");
    }

    #[test]
    fn arabic_is_shaped_and_reversed() {
        // "فتح" = FEH, TEH, HAH → visual: HAH final, TEH medial, FEH initial.
        let out = visual("فتح", Some(true));
        assert_eq!(out, "\u{FEA2}\u{FE98}\u{FED3}");
    }

    #[test]
    fn mixed_runs_keep_latin_order() {
        // RTL paragraph: Latin run stays readable, Arabic runs reversed.
        let out = visual("مفتاح SubDL", Some(true));
        assert!(out.starts_with("SubDL "), "{out:?}");
    }

    #[test]
    fn brackets_are_mirrored_in_rtl() {
        let out = visual("(خارجي)", Some(true));
        assert!(out.starts_with('(') && out.ends_with(')'), "{out:?}");
    }

    #[test]
    fn formatting_fills_placeholders() {
        set_language(Lang::English);
        assert_eq!(tlf("Volume {}", &[&50]), "Volume 50");
        set_language(Lang::Arabic);
        assert_eq!(tlf("Volume {}", &[&50]), "حجم الصوت 50");
        set_language(Lang::English);
    }
}
