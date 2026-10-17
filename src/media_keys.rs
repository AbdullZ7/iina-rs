//! System media controls: the keyboard's media keys, the Windows media
//! overlay / lock screen (SMTC), MPRIS on Linux and Now Playing on macOS —
//! IINA's `MPNowPlayingInfoCenter` / `MPRemoteCommandCenter` integration.


/// A request from the system, handled on the UI thread.
/// (Toggle, Raise and Quit come from MPRIS / Now Playing, not Windows.)
#[derive(Debug, Clone)]
#[cfg_attr(windows, allow(dead_code))]
pub enum MediaCmd {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    /// Relative seek in seconds.
    SeekBy(f64),
    /// Absolute position in seconds.
    SetPosition(f64),
    Raise,
    Quit,
}

#[derive(Default, PartialEq, Clone)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub idle: bool,
    pub paused: bool,
}

pub use imp::Media;

#[cfg(not(windows))]
mod imp {
    use super::*;
    use std::time::Duration;
    use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};

    pub struct Media {
        controls: MediaControls,
        shown: Option<NowPlaying>,
    }

    impl Media {
        /// `hwnd` is required on Windows (the main window's HWND).
        pub fn new(hwnd: Option<*mut std::ffi::c_void>, send: impl Fn(MediaCmd) + Send + 'static) -> Option<Self> {
            let config = PlatformConfig { dbus_name: "iina_rs", display_name: "IINA-RS", hwnd };
            let mut controls = MediaControls::new(config).map_err(|e| eprintln!("media controls: {e:?}")).ok()?;
            controls
                .attach(move |ev| {
                    let cmd = match ev {
                        MediaControlEvent::Play => MediaCmd::Play,
                        MediaControlEvent::Pause => MediaCmd::Pause,
                        MediaControlEvent::Toggle => MediaCmd::Toggle,
                        MediaControlEvent::Next => MediaCmd::Next,
                        MediaControlEvent::Previous => MediaCmd::Previous,
                        MediaControlEvent::Stop => MediaCmd::Stop,
                        MediaControlEvent::Seek(d) => MediaCmd::SeekBy(if matches!(d, SeekDirection::Forward) { 10.0 } else { -10.0 }),
                        MediaControlEvent::SeekBy(d, by) => {
                            let s = by.as_secs_f64();
                            MediaCmd::SeekBy(if matches!(d, SeekDirection::Forward) { s } else { -s })
                        }
                        MediaControlEvent::SetPosition(MediaPosition(p)) => MediaCmd::SetPosition(p.as_secs_f64()),
                        MediaControlEvent::Raise => MediaCmd::Raise,
                        MediaControlEvent::Quit => MediaCmd::Quit,
                        _ => return,
                    };
                    send(cmd);
                })
                .map_err(|e| eprintln!("media controls attach: {e:?}"))
                .ok()?;
            Some(Media { controls, shown: None })
        }

        /// Publishes what's playing. `position` is sent on every state change
        /// so the system timeline stays in step after seeks.
        pub fn update(&mut self, now: &NowPlaying, position: f64, force_position: bool) {
            let changed = self.shown.as_ref() != Some(now);
            if changed && self.shown.as_ref().is_none_or(|s| s.title != now.title || s.artist != now.artist || s.album != now.album || s.duration != now.duration) {
                let opt = |s: &str| (!s.is_empty()).then_some(s.to_string());
                let (title, artist, album) = (opt(&now.title), opt(&now.artist), opt(&now.album));
                let _ = self.controls.set_metadata(MediaMetadata {
                    title: title.as_deref(),
                    artist: artist.as_deref(),
                    album: album.as_deref(),
                    cover_url: None,
                    duration: (now.duration > 0.0).then(|| Duration::from_secs_f64(now.duration)),
                });
            }
            if changed || force_position {
                let progress = Some(MediaPosition(Duration::from_secs_f64(position.max(0.0))));
                let state = if now.idle {
                    MediaPlayback::Stopped
                } else if now.paused {
                    MediaPlayback::Paused { progress }
                } else {
                    MediaPlayback::Playing { progress }
                };
                let _ = self.controls.set_playback(state);
            }
            self.shown = Some(now.clone());
        }
    }
}

/// Windows: the System Media Transport Controls of our window (media keys,
/// the volume flyout and the lock screen), straight from WinRT.
#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::Arc;
    use windows::core::{factory, HSTRING};
    use windows::Foundation::{TimeSpan, TypedEventHandler};
    use windows::Media::{
        MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs, SystemMediaTransportControls,
        SystemMediaTransportControlsButton as Button, SystemMediaTransportControlsButtonPressedEventArgs, SystemMediaTransportControlsTimelineProperties,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;

    pub struct Media {
        smtc: SystemMediaTransportControls,
        shown: Option<NowPlaying>,
    }

    fn span(secs: f64) -> TimeSpan {
        TimeSpan { Duration: (secs.max(0.0) * 1e7) as i64 }
    }

    impl Media {
        pub fn new(hwnd: Option<*mut std::ffi::c_void>, send: impl Fn(MediaCmd) + Send + Sync + 'static) -> Option<Self> {
            let hwnd = HWND(hwnd?);
            let result = (|| -> windows::core::Result<Self> {
                let interop = factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
                let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd)? };
                smtc.SetIsEnabled(true)?;
                smtc.SetIsPlayEnabled(true)?;
                smtc.SetIsPauseEnabled(true)?;
                smtc.SetIsStopEnabled(true)?;
                smtc.SetIsNextEnabled(true)?;
                smtc.SetIsPreviousEnabled(true)?;
                smtc.SetIsFastForwardEnabled(true)?;
                smtc.SetIsRewindEnabled(true)?;
                let send = Arc::new(send);
                let s = send.clone();
                smtc.ButtonPressed(&TypedEventHandler::<SystemMediaTransportControls, SystemMediaTransportControlsButtonPressedEventArgs>::new(move |_, args| {
                    let cmd = match args.ok()?.Button()? {
                        Button::Play => MediaCmd::Play,
                        Button::Pause => MediaCmd::Pause,
                        Button::Stop => MediaCmd::Stop,
                        Button::Next => MediaCmd::Next,
                        Button::Previous => MediaCmd::Previous,
                        Button::FastForward => MediaCmd::SeekBy(10.0),
                        Button::Rewind => MediaCmd::SeekBy(-10.0),
                        _ => return Ok(()),
                    };
                    s(cmd);
                    Ok(())
                }))?;
                let s = send.clone();
                smtc.PlaybackPositionChangeRequested(&TypedEventHandler::<SystemMediaTransportControls, PlaybackPositionChangeRequestedEventArgs>::new(move |_, args| {
                    let t = args.ok()?.RequestedPlaybackPosition()?;
                    s(MediaCmd::SetPosition(t.Duration as f64 / 1e7));
                    Ok(())
                }))?;
                Ok(Media { smtc, shown: None })
            })();
            result.map_err(|e| eprintln!("media controls: {e}")).ok()
        }

        pub fn update(&mut self, now: &NowPlaying, position: f64, force_position: bool) {
            let changed = self.shown.as_ref() != Some(now);
            let _ = (|| -> windows::core::Result<()> {
                if changed && self.shown.as_ref().is_none_or(|s| s.title != now.title || s.artist != now.artist || s.album != now.album) {
                    let du = self.smtc.DisplayUpdater()?;
                    du.ClearAll()?;
                    if !now.artist.is_empty() || !now.album.is_empty() {
                        du.SetType(MediaPlaybackType::Music)?;
                        let m = du.MusicProperties()?;
                        m.SetTitle(&HSTRING::from(now.title.as_str()))?;
                        m.SetArtist(&HSTRING::from(now.artist.as_str()))?;
                        m.SetAlbumTitle(&HSTRING::from(now.album.as_str()))?;
                    } else {
                        du.SetType(MediaPlaybackType::Video)?;
                        du.VideoProperties()?.SetTitle(&HSTRING::from(now.title.as_str()))?;
                    }
                    du.Update()?;
                }
                if changed || force_position {
                    self.smtc.SetPlaybackStatus(if now.idle {
                        MediaPlaybackStatus::Stopped
                    } else if now.paused {
                        MediaPlaybackStatus::Paused
                    } else {
                        MediaPlaybackStatus::Playing
                    })?;
                    let tl = SystemMediaTransportControlsTimelineProperties::new()?;
                    tl.SetStartTime(span(0.0))?;
                    tl.SetMinSeekTime(span(0.0))?;
                    tl.SetEndTime(span(now.duration))?;
                    tl.SetMaxSeekTime(span(now.duration))?;
                    tl.SetPosition(span(position.min(now.duration.max(position))))?;
                    self.smtc.UpdateTimelineProperties(&tl)?;
                }
                Ok(())
            })();
            self.shown = Some(now.clone());
        }
    }
}
