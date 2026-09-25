//! The music: a live station, decoded here and played through whatever the
//! machine plays sound with.
//!
//! A station rather than a loop made up on the spot. A party lasts longer than
//! any loop stays bearable, and a DJ who never plays the same track twice is a
//! better DJ than eight bars of synthesiser played forever. It is SomaFM's The
//! Trip — listener-supported, free to stream, and progressive house and trance,
//! which is what the lights are dancing to.
//!
//! No audio output crate: every one of them links the system's sound library,
//! and cctop ships as a static musl binary with no system library to link. The
//! decoding is Symphonia, which is pure Rust and so links into anything. What
//! every Linux desktop does have is a command that plays raw samples from its
//! standard input — `pacat` for PulseAudio, which WSLg provides and PipeWire
//! answers for through `pipewire-pulse`, and `aplay` for bare ALSA — so the
//! decoded stream is written into the first of those that exists. `pw-cat` is
//! not among them: it will not take raw samples from a pipe, only a file whose
//! header says what they are.
//!
//! Offline, or with nothing to play it through, the party is silent rather
//! than an error: the lights never depended on the music.
//!
//! ponytail: the sound plays where cctop runs. Over ssh that is the far
//! machine's speakers, if it has any, and nobody hears it.
//!
//! ponytail: the station is not on the lights' beat. The lights keep their own
//! 128, and no tempo is read out of the music to follow.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use symphonia::core::audio::sample::Sample;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;

/// What is playing, for the line that says so.
pub const STATION: &str = "SomaFM · The Trip";

const STREAM: &str = "https://ice2.somafm.com/thetrip-128-mp3";

/// A station being played, stopped when this is dropped.
#[derive(Debug)]
pub struct Sound {
    stop: Arc<AtomicBool>,
    player: Arc<Mutex<Option<Child>>>,
}

impl Sound {
    /// Tune in. The connection is made on a thread of its own, so a slow or
    /// absent network never holds up the key that started the party.
    ///
    /// Never under test: a test that starts the party would otherwise play
    /// the radio through the speakers of whoever runs `cargo test`.
    pub fn start() -> Option<Self> {
        if cfg!(test) {
            return None;
        }
        let sound = Self {
            stop: Arc::new(AtomicBool::new(false)),
            player: Arc::new(Mutex::new(None)),
        };
        let (stop, player) = (sound.stop.clone(), sound.player.clone());
        std::thread::Builder::new()
            .name("rave-sound".into())
            .spawn(move || {
                let _ = play(&stop, &player);
            })
            .ok()?;
        Some(sound)
    }
}

impl Drop for Sound {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Killing the player is also what unblocks a thread parked writing
        // to it: its next write fails, and it returns.
        if let Some(mut child) = self.player.lock().ok().and_then(|mut p| p.take()) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Stream, decode and play until told to stop, or until anything fails.
fn play(stop: &AtomicBool, player: &Mutex<Option<Child>>) -> Option<()> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(5)))
        .user_agent(concat!("cctop/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let body = agent.get(STREAM).call().ok()?.into_body().into_reader();
    let source = ReadOnlySource::new(Shared(Mutex::new(body)));
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .ok()?;
    let track = format.default_track(TrackType::Audio)?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(
            track.codec_params.as_ref()?.audio()?,
            &AudioDecoderOptions::default(),
        )
        .ok()?;

    let mut pipe = None;
    let mut samples: Vec<i16> = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        let packet = format.next_packet().ok()??;
        if packet.track_id != track_id {
            continue;
        }
        // A corrupt frame is a skipped frame: a radio stream has them.
        let Ok(audio) = decoder.decode(&packet) else {
            continue;
        };
        // The player is started on the first frame, which is the first
        // moment the rate and the channel count are known.
        if pipe.is_none() {
            let spec = audio.spec();
            let mut child = spawn_player(spec.rate(), spec.channels().count())?;
            pipe = child.stdin.take();
            let mut slot = player.lock().ok()?;
            // Stopped while tuning in: this player is nobody's to kill.
            if stop.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            *slot = Some(child);
        }
        samples.resize(audio.samples_interleaved(), i16::MID);
        audio.copy_to_slice_interleaved(&mut samples);
        bytes.clear();
        bytes.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        pipe.as_mut()?.write_all(&bytes).ok()?;
    }
    Some(())
}

/// The first player that starts, reading signed 16-bit samples at `rate` from
/// standard input.
///
/// With a buffer of a second and a half, which is long for audio and right for
/// this. The music follows nothing on screen, so it has no latency to keep
/// short, while what it plays through can stall: WSLg's sink forwards sound
/// to Windows over the remote-desktop channel, which was measured running a
/// second and more behind, and a 200ms buffer there was half a second of
/// music, then a gap, then half a second again.
fn spawn_player(rate: u32, channels: usize) -> Option<Child> {
    let (rate, channels) = (rate.to_string(), channels.to_string());
    let players: [(&str, Vec<String>); 2] = [
        (
            "pacat",
            vec![
                "--playback".into(),
                "--raw".into(),
                "--format=s16le".into(),
                format!("--rate={rate}"),
                format!("--channels={channels}"),
                "--latency-msec=1500".into(),
                "--client-name=cctop".into(),
                format!("--stream-name={STATION}"),
            ],
        ),
        (
            "aplay",
            vec![
                "-q".into(),
                "-t".into(),
                "raw".into(),
                "-f".into(),
                "S16_LE".into(),
                "-r".into(),
                rate,
                "-c".into(),
                channels,
                "--buffer-time=1500000".into(),
            ],
        ),
    ];
    players.iter().find_map(|(name, args)| {
        Command::new(name)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()
    })
}

/// A reader that can be shared between threads, which is what Symphonia asks
/// of a source; the body reader is only ever read from the one thread, so the
/// lock is never contended.
struct Shared<R>(Mutex<R>);

impl<R: Read> Read for Shared<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.0.get_mut() {
            Ok(inner) => inner.read(buf),
            Err(_) => Err(std::io::Error::other("poisoned")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test can start a party without the radio coming on.
    #[test]
    fn no_music_under_test() {
        assert!(Sound::start().is_none());
    }
}
