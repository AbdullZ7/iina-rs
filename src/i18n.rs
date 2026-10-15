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
    ("Mute", "كتم الصوت"),
    ("Volume {}", "مستوى الصوت {}"),
    ("Previous", "السابق"),
    ("Play", "تشغيل"),
    ("Pause", "إيقاف مؤقت"),
    ("Next", "التالي"),
    ("Fullscreen", "ملء الشاشة"),
    ("Picture-in-Picture", "صورة داخل صورة"),
    ("Playlist", "قائمة التشغيل"),
    ("Subtitles", "الترجمات"),
    ("Video & Audio settings", "إعدادات الفيديو والصوت"),
    ("Click to toggle remaining time", "انقر للتبديل إلى الوقت المتبقي"),
    // Welcome
    ("Version {}", "الإصدار {}"),
    ("Open File…", "فتح ملف…"),
    ("Open URL…", "فتح رابط…"),
    ("Recent", "المفتوحة مؤخراً"),
    ("Drop a video here, or open a file.", "اسحب فيديو إلى هنا، أو افتح ملفاً."),
    ("Clear recent", "مسح القائمة"),
    // Sidebar
    ("None", "بلا"),
    ("(external)", "(خارجي)"),
    ("Reset", "إعادة ضبط"),
    ("Flat", "مسطّح"),
    ("Bass Boost", "تعزيز الجهير"),
    ("Treble Boost", "تعزيز الحادّ"),
    ("Vocal", "صوتي"),
    ("Rock", "روك"),
    ("Loudness", "جهارة"),
    ("Chapters", "الفصول"),
    ("Video", "الفيديو"),
    ("Audio", "الصوت"),
    ("No chapters", "لا توجد فصول"),
    ("Chapter {}", "الفصل {}"),
    ("Subtitle track", "مسار الترجمة"),
    ("Load external subtitle…", "تحميل ترجمة خارجية…"),
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
    ("Add files…", "إضافة ملفات…"),
    ("Shuffle", "خلط"),
    ("Loop", "تكرار"),
    ("Move Up", "نقل لأعلى"),
    ("Move Down", "نقل لأسفل"),
    ("Remove", "إزالة"),
    ("Show in Folder", "إظهار في المجلد"),
    ("Video track", "مسار الفيديو"),
    ("Speed", "السرعة"),
    ("Aspect ratio", "نسبة الأبعاد"),
    ("Default", "افتراضي"),
    ("Crop", "القصّ"),
    ("Rotation", "التدوير"),
    ("Flip", "قلب عمودي"),
    ("Mirror", "انعكاس أفقي"),
    ("Deinterlace", "إزالة التشابك"),
    ("Video equalizer", "معادل الصورة"),
    ("Brightness", "السطوع"),
    ("Contrast", "التباين"),
    ("Saturation", "التشبّع"),
    ("Gamma", "جاما"),
    ("Hue", "درجة اللون"),
    ("Reset equalizer", "إعادة ضبط المعادل"),
    ("Hardware decoding", "فك الترميز بالعتاد"),
    ("Decoder: {}", "مفكك الترميز: {}"),
    ("software", "برمجي"),
    ("Take screenshot", "التقاط صورة للشاشة"),
    ("Audio track", "مسار الصوت"),
    ("Load external audio…", "تحميل صوت خارجي…"),
    ("Volume", "مستوى الصوت"),
    ("Audio delay", "تأخير الصوت"),
    ("Equalizer", "المعادل"),
    // Context menu
    ("Jump to…", "انتقال إلى…"),
    ("Find Online Subtitles…", "البحث عن ترجمات على الإنترنت…"),
    ("Take Screenshot", "التقاط صورة للشاشة"),
    ("Stop", "إيقاف"),
    ("Video Settings", "إعدادات الفيديو"),
    ("Audio Settings", "إعدادات الصوت"),
    ("Subtitle Settings", "إعدادات الترجمة"),
    ("Exit Fullscreen", "الخروج من ملء الشاشة"),
    ("Enter Fullscreen", "ملء الشاشة"),
    ("✓ Float on Top", "✓ فوق كل النوافذ"),
    ("Float on Top", "فوق كل النوافذ"),
    ("✓ Picture-in-Picture", "✓ صورة داخل صورة"),
    ("✓ Music Mode", "✓ وضع الموسيقى"),
    ("Music Mode", "وضع الموسيقى"),
    ("Inspector", "المفتّش"),
    ("Playback History", "سجل التشغيل"),
    ("Preferences…", "التفضيلات…"),
    ("About IINA-RS", "عن IINA-RS"),
    ("Quit", "إنهاء"),
    // Dialogs
    ("Open URL", "فتح رابط"),
    ("Enter a media URL (http, https, YouTube via yt-dlp, …):", "أدخل رابط وسائط (http أو https أو يوتيوب عبر yt-dlp…):"),
    ("Open", "فتح"),
    ("Cancel", "إلغاء"),
    ("Jump to", "انتقال إلى"),
    ("Time (hh:mm:ss) or percentage — duration {}", "الوقت (ساعة:دقيقة:ثانية) أو نسبة مئوية — المدة {}"),
    ("1:23:45  or  50%", "1:23:45 أو 50%"),
    ("Jump", "انتقال"),
    // Inspector
    ("Nothing is playing.", "لا شيء قيد التشغيل."),
    ("General", "عام"),
    ("Path", "المسار"),
    ("Format", "الصيغة"),
    ("Size", "الحجم"),
    ("Duration", "المدة"),
    ("Codec", "الترميز"),
    ("Resolution", "الدقة"),
    ("Display", "العرض"),
    ("Pixel format", "صيغة البكسل"),
    ("Colors", "الألوان"),
    ("Frame rate", "معدل الإطارات"),
    ("Bitrate", "معدل البت"),
    ("Hardware decoder", "مفكك العتاد"),
    ("Sample rate", "معدل العينات"),
    ("Channels", "القنوات"),
    ("Playback", "التشغيل"),
    ("Dropped (VO / decoder)", "الإطارات المُسقطة (العرض / المفكك)"),
    ("A/V sync", "تزامن الصوت والصورة"),
    ("Cache", "التخزين المؤقت"),
    // Preferences
    ("Preferences", "التفضيلات"),
    ("Language", "اللغة"),
    ("Automatic", "تلقائي"),
    ("Resume last playback position", "استئناف التشغيل من آخر موضع"),
    ("Add files in the same folder to the playlist", "إضافة ملفات المجلد نفسه إلى قائمة التشغيل"),
    ("Resize window to video size when opening a file", "ملاءمة حجم النافذة للفيديو عند فتح ملف"),
    ("Open files in the running window (single instance)", "فتح الملفات في النافذة المفتوحة (نسخة واحدة)"),
    ("UI", "الواجهة"),
    ("Show thumbnail preview on the seek bar", "إظهار معاينة مصغّرة على شريط التقدّم"),
    ("Show remaining time", "إظهار الوقت المتبقي"),
    ("Single click on video toggles pause", "نقرة واحدة على الفيديو توقفه مؤقتاً"),
    ("Hide controller after", "إخفاء أزرار التحكم بعد"),
    ("Video / Audio", "الفيديو / الصوت"),
    ("Enable hardware decoding", "تفعيل فك الترميز بالعتاد"),
    ("Initial volume", "مستوى الصوت الابتدائي"),
    ("Screenshots", "لقطات الشاشة"),
    ("Choose…", "اختيار…"),
    ("Online subtitles", "الترجمات على الإنترنت"),
    ("SubDL key", "مفتاح SubDL"),
    ("Get a key", "احصل على مفتاح"),
    ("OpenSubtitles key", "مفتاح OpenSubtitles"),
    ("Languages", "اللغات"),
    ("System", "النظام"),
    ("Add to “Open with”", "إضافة إلى «فتح باستخدام»"),
    ("Registers IINA-RS for video/audio files for your user account", "يسجّل IINA-RS لملفات الفيديو والصوت في حسابك"),
    ("Default apps…", "التطبيقات الافتراضية…"),
    ("Open settings folder", "فتح مجلد الإعدادات"),
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
    ("Clear History", "مسح السجل"),
    ("No history yet.", "لا يوجد سجل بعد."),
    ("Remove from History", "إزالة من السجل"),
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
    ("Open Preferences", "فتح التفضيلات"),
    ("Title", "العنوان"),
    ("Search", "بحث"),
    ("(no release name)", "(بدون اسم إصدار)"),
    ("Likely synced to this file", "غالباً متزامنة مع هذا الملف"),
    ("Matched by title", "مطابقة بالعنوان"),
    ("By {}", "بواسطة {}"),
    ("Hearing impaired (SDH)", "لضعاف السمع (SDH)"),
    ("Download", "تنزيل"),
    ("Searching {}…", "جارٍ البحث في {}…"),
    ("No subtitles found.", "لم يُعثر على ترجمات."),
    ("{} results from {}", "{} نتيجة من {}"),
    ("Downloading…", "جارٍ التنزيل…"),
    ("Loaded {}", "تم تحميل {}"),
    ("{} rejected the API key. Check it in Preferences.", "رفض {} مفتاح API. تحقّق منه في التفضيلات."),
    ("Network error: {}", "خطأ في الشبكة: {}"),
    ("The archive contains no subtitle file.", "لا يحتوي الملف المضغوط على ترجمة."),
    // On-screen messages
    ("Volume: {}", "مستوى الصوت: {}"),
    ("Unmute", "إلغاء الكتم"),
    ("Speed: {}x", "السرعة: {}x"),
    ("Resume", "استئناف"),
    ("Subtitle Delay: {}s", "تأخير الترجمة: {} ث"),
    ("Audio Delay: {}s", "تأخير الصوت: {} ث"),
    ("Subtitle: {}", "الترجمة: {}"),
    ("Secondary Subtitle: {}", "الترجمة الثانوية: {}"),
    ("Audio: {}", "الصوت: {}"),
    ("Video: {}", "الفيديو: {}"),
    ("Subtitle Scale: {}x", "حجم الترجمة: {}x"),
    ("Subtitle Position: {}", "موضع الترجمة: {}"),
    ("Subtitles Hidden", "الترجمة مخفية"),
    ("Subtitles Visible", "الترجمة ظاهرة"),
    ("Loop: File", "التكرار: الملف"),
    ("Loop: Playlist", "التكرار: قائمة التشغيل"),
    ("Loop: Off", "التكرار: متوقف"),
    ("A-B Loop: A = {}", "تكرار A-B: البداية {}"),
    ("A-B Loop: {} – {}", "تكرار A-B: {} – {}"),
    ("A-B Loop: Off", "تكرار A-B: متوقف"),
    ("Aspect Ratio: {}", "نسبة الأبعاد: {}"),
    ("Rotation: {}°", "التدوير: {}°"),
    ("Deinterlace: {}", "إزالة التشابك: {}"),
    ("Hardware Decoding: {}", "فك الترميز بالعتاد: {}"),
    ("On", "مفعّل"),
    ("Off", "متوقف"),
    ("Screenshot Captured", "تم التقاط الصورة"),
    ("Chapter: {}", "الفصل: {}"),
    ("HDR output on HDR displays (restart required)", "إخراج HDR على الشاشات الداعمة (يتطلب إعادة التشغيل)"),
    ("Output", "المخرج"),
    // Subtitle preferences
    ("Preferred languages", "اللغات المفضّلة"),
    ("Font", "الخط"),
    ("Default font", "الخط الافتراضي"),
    ("Search fonts…", "ابحث عن خط…"),
    ("Loading fonts…", "جارٍ تحميل الخطوط…"),
    ("Bold", "عريض"),
    ("Size", "الحجم"),
    ("Text", "النص"),
    ("Outline", "الحد"),
    ("Shadow", "الظل"),
    ("Letter spacing", "تباعد الحروف"),
    ("Style", "النمط"),
    ("Outline & shadow", "حد وظل"),
    ("Opaque box", "صندوق معتم"),
    ("Background box", "صندوق خلفية"),
    ("Box color", "لون الصندوق"),
    ("Vertical position", "الموضع العمودي"),
    ("Bottom margin", "الهامش السفلي"),
    ("Secondary subtitle position", "موضع الترجمة الثانوية"),
    ("Encoding", "الترميز"),
    ("Auto-detect", "اكتشاف تلقائي"),
    ("Arabic (Windows-1256)", "العربية (Windows-1256)"),
    ("ASS/SSA styling", "تنسيق ASS/SSA"),
    ("As authored", "كما في الملف"),
    ("Use my style", "استخدم تنسيقي"),
    ("Remove styling", "إزالة التنسيق"),
    ("Load external subtitles", "تحميل الترجمات الخارجية"),
    ("Exact file name", "نفس اسم الملف"),
    ("Containing the file name", "تحتوي اسم الملف"),
    ("Any in the folder", "أي ترجمة في المجلد"),
    ("Show text subtitles in the black bars", "إظهار الترجمة النصية في الأشرطة السوداء"),
    ("Remove hearing-impaired text", "إزالة نصوص ضعاف السمع"),
    ("Sync steps (s)", "خطوات المزامنة (ث)"),
    ("Open fonts folder", "فتح مجلد الخطوط"),
    ("Shortcuts: Z/X, Ctrl+Shift+Z/X, Ctrl+Alt+Shift+Z/X", "الاختصارات: Z/X و Ctrl+Shift+Z/X و Ctrl+Alt+Shift+Z/X"),
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
        assert_eq!(tlf("Volume {}", &[&50]), "مستوى الصوت 50");
        set_language(Lang::English);
    }
}
