//! Media editor for SnapPea.
//!
//! Opens a recorded GIF or video, displays a preview with a scrubber timeline,
//! and lets the user trim, scale, and export as GIF or video.
//!
//! - **GIF files**: decoded into frames, played via GifPlayer widget
//! - **Video files**: played natively via GStreamer (iced_video_player), no frames in memory
//!
//! Usage: snappea-edit [--discard] <path-to-media>

use cosmetics::widgets::gif_player::{self, Frames as GifFrames};
use cosmetics::widgets::toggle::Toggle;
use cosmic::{
    Application,
    app::{Settings, Task},
    executor,
    iced::clipboard::mime::AsMimeTypes,
    iced::{Alignment, ContentFit, Length},
    widget,
    widget::icon,
};
use iced_video_player::{Video, VideoPlayer};
use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::{AnimationDecoder, Frame, RgbaImage};
use snappea::fl;

#[path = "../widget/video_scrubber.rs"]
mod video_scrubber;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;
use video_scrubber::video_scrubber;

// ── Types ─────────────────────────────────────────────────────────────────

const SPEED_OPTIONS: &[f64] = &[0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0];
const SPEED_LABELS: &[&str] = &["0.25x", "0.5x", "0.75x", "1x", "1.5x", "2x", "4x"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaType {
    Gif,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Gif,
    Video,
}

fn detect_media_type(path: &Path) -> MediaType {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .as_deref()
    {
        Some("gif") => MediaType::Gif,
        _ => MediaType::Video,
    }
}

/// What's loaded and how it's played back.
enum MediaState {
    /// GIF: frames in memory, GifPlayer widget
    Gif {
        frames: Vec<RgbaImage>,
        gif_frames: GifFrames,
        delay_ms: u32,
    },
    /// Video: GStreamer playback, no frames in memory
    VideoLoaded { video: Video },
    /// Waiting for file to appear (encoding in progress)
    Loading,
}

// ── Clipboard ─────────────────────────────────────────────────────────────

struct ClipboardData {
    png_bytes: Vec<u8>,
    gif_bytes: Vec<u8>,
    file_uri: String,
}

impl AsMimeTypes for ClipboardData {
    fn available(&self) -> Cow<'static, [String]> {
        let mut types = Vec::new();
        if !self.png_bytes.is_empty() {
            types.push("image/png".to_string());
        }
        if !self.gif_bytes.is_empty() {
            types.push("image/gif".to_string());
        }
        types.push("text/uri-list".to_string());
        Cow::Owned(types)
    }

    fn as_bytes(&self, mime_type: &str) -> Option<Cow<'static, [u8]>> {
        match mime_type {
            "image/png" if !self.png_bytes.is_empty() => Some(Cow::Owned(self.png_bytes.clone())),
            "image/gif" if !self.gif_bytes.is_empty() => Some(Cow::Owned(self.gif_bytes.clone())),
            "text/uri-list" => Some(Cow::Owned(format!("{}\r\n", self.file_uri).into_bytes())),
            _ => None,
        }
    }
}

// ── Main ──────────────────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    snappea::localize::localize();

    let args: Vec<String> = std::env::args().collect();
    let can_discard = args.iter().any(|a| a == "--discard");
    let media_path = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| {
            eprintln!("Usage: snappea-edit [--discard] <path-to-media>");
            std::process::exit(1);
        });

    let media_path = PathBuf::from(media_path);
    let media_type = detect_media_type(&media_path);

    let settings = Settings::default()
        .size(cosmic::iced::Size::new(700.0, 500.0))
        .debug(false);

    cosmic::app::run::<MediaEditor>(
        settings,
        Flags {
            media_path,
            media_type,
            can_discard,
        },
    )?;

    Ok(())
}

struct Flags {
    media_path: PathBuf,
    media_type: MediaType,
    can_discard: bool,
}

// ── App state ─────────────────────────────────────────────────────────────

struct MediaEditor {
    core: cosmic::Core,
    media_path: PathBuf,
    media_type: MediaType,
    media: MediaState,
    _temp_dir: tempfile::TempDir,

    // Timeline (shared by both modes, in seconds)
    duration: f64,
    position: f64,
    trim_start: f64,
    trim_end: f64,
    playing: bool,
    dragging: bool,

    // Cut editing
    cuts: Vec<f64>,
    selected_chunk: Option<usize>,
    deleted_chunks: HashSet<usize>,
    chunk_speeds: HashMap<usize, f64>,
    frame_colors: Vec<[u8; 3]>,

    // UI state
    loaded: bool,
    poll_count: usize,
    status: String,
    can_discard: bool,

    // Output settings
    export_format: ExportFormat,
    output_scale: u32,
    output_fps: Option<u32>,
    ffmpeg_available: bool,
    ffmpeg_version: Option<String>,
    gifski_available: bool,
    gifski_version: Option<String>,
    use_ffmpeg: bool,
    file_size_bytes: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Message {
    // Timeline
    Seek(f64),
    SeekRelease,
    TrimChanged((f64, f64)),
    // Cut editing
    CutAtPlayhead,
    SelectChunk(Option<usize>),
    DeleteSelectedChunk,
    SetChunkSpeed(usize),
    MoveCut(usize, f64),
    AddCut(f64),
    RemoveCut(usize),
    ColorsExtracted(Vec<[u8; 3]>),
    // Playback
    TogglePlay,
    NewFrame,
    GifFrameChanged(usize),
    DurationChanged(Duration),
    EndOfStream,
    // Loading
    PollFile,
    GifLoaded(Vec<(u32, u32, Vec<u8>)>, u32, f64),
    VideoReady,
    // Output settings
    SetExportFormat(usize),
    SetOutputScale(usize),
    SetOutputFps(usize),
    SetUseFfmpeg(bool),
    // Actions
    Save,
    SaveAs,
    SaveAsChosen(Option<PathBuf>),
    CopyToClipboard,
    CopyDone(Result<(), String>),
    Discard,
    // About
    ToggleAbout,
}

// ── GIF helpers ───────────────────────────────────────────────────────────

fn load_gif(path: &Path) -> anyhow::Result<(Vec<RgbaImage>, u32)> {
    let file = BufReader::new(File::open(path)?);
    let decoder = GifDecoder::new(file)?;
    let raw_frames: Vec<Frame> = decoder.into_frames().collect_frames()?;

    let mut delay_ms = 100u32;
    let mut images = Vec::with_capacity(raw_frames.len());

    for (i, frame) in raw_frames.iter().enumerate() {
        let (numer, denom) = frame.delay().numer_denom_ms();
        let ms: u32 = if denom > 0 { numer / denom } else { 100 };
        if i == 0 {
            delay_ms = ms;
        }
        images.push(frame.buffer().clone());
    }

    Ok((images, delay_ms))
}

fn build_gif_frames(frames: &[RgbaImage], delay_ms: u32) -> GifFrames {
    const MAX_SYNC_BYTES: usize = 2 * 1024 * 1024;
    const BPP: usize = 4;

    let needs_downscale = frames.first().map_or(false, |f| {
        (f.width() as usize * f.height() as usize * BPP) > MAX_SYNC_BYTES
    });

    if needs_downscale {
        let first = &frames[0];
        let pixels = first.width() as usize * first.height() as usize;
        let scale = (MAX_SYNC_BYTES as f64 / BPP as f64 / pixels as f64).sqrt();
        let new_w = ((first.width() as f64 * scale) as u32).max(1);
        let new_h = ((first.height() as f64 * scale) as u32).max(1);

        let scaled: Vec<RgbaImage> = frames
            .iter()
            .map(|img| {
                image::imageops::resize(img, new_w, new_h, image::imageops::FilterType::Triangle)
            })
            .collect();

        let images: Vec<(u32, u32, &[u8])> = scaled
            .iter()
            .map(|img| (img.width(), img.height(), img.as_raw().as_slice()))
            .collect();
        GifFrames::from_rgba(&images, delay_ms)
    } else {
        let images: Vec<(u32, u32, &[u8])> = frames
            .iter()
            .map(|img| (img.width(), img.height(), img.as_raw().as_slice()))
            .collect();
        GifFrames::from_rgba(&images, delay_ms)
    }
}

// ── Export helpers ─────────────────────────────────────────────────────────

fn save_trimmed_gif(
    frames: &[RgbaImage],
    delay_ms: u32,
    segments: &[(f64, f64, f64)],
    duration: f64,
    output_scale: u32,
    output_fps: Option<u32>,
    use_ffmpeg: bool,
    output_path: &Path,
) -> anyhow::Result<u64> {
    let file = File::create(output_path)?;
    let mut encoder = GifEncoder::new_with_speed(file, 10);
    encoder.set_repeat(Repeat::Infinite)?;

    let total = frames.len();
    let time_to_frame = |t: f64| -> usize {
        if duration <= 0.0 || total == 0 {
            return 0;
        }
        ((t / duration * total as f64) as usize).min(total.saturating_sub(1))
    };

    let original_fps = 1000.0 / delay_ms.max(1) as f64;

    struct FrameEntry<'a> {
        image: &'a RgbaImage,
        delay_ms: u32,
    }

    let mut kept: Vec<FrameEntry<'_>> = Vec::new();
    for &(seg_start, seg_end, speed) in segments {
        let f_start = time_to_frame(seg_start);
        let f_end = time_to_frame(seg_end).min(total);
        let seg_delay = ((delay_ms as f64) / speed).round().max(1.0) as u32;

        let (step, actual_delay) = match output_fps {
            Some(fps) if (fps as f64) < original_fps * speed => {
                let s = (original_fps * speed / fps as f64).round() as usize;
                (s.max(1), seg_delay * s.max(1) as u32)
            }
            _ => (1, seg_delay),
        };

        for f in frames[f_start..f_end].iter().step_by(step) {
            kept.push(FrameEntry {
                image: f,
                delay_ms: actual_delay,
            });
        }
    }

    if kept.is_empty() {
        return Err(anyhow::anyhow!("No frames to export"));
    }

    let (out_w, out_h) = if output_scale < 100 {
        let f = kept[0].image;
        let s = output_scale as f64 / 100.0;
        (
            ((f.width() as f64 * s) as u32).max(1),
            ((f.height() as f64 * s) as u32).max(1),
        )
    } else {
        (0, 0)
    };

    for entry in &kept {
        let img = if out_w > 0 && out_h > 0 {
            image::imageops::resize(
                entry.image,
                out_w,
                out_h,
                image::imageops::FilterType::Triangle,
            )
        } else {
            entry.image.clone()
        };
        let delay = image::Delay::from_numer_denom_ms(entry.delay_ms, 1);
        encoder.encode_frame(Frame::from_parts(img, 0, 0, delay))?;
    }
    drop(encoder);

    if use_ffmpeg {
        let opt = output_path.with_extension("opt.gif");
        let r = std::process::Command::new("ffmpeg")
            .args(["-y", "-i"])
            .arg(output_path)
            .args([
                "-vf",
                "split[s0][s1];[s0]palettegen=max_colors=128[p];[s1][p]paletteuse=dither=bayer",
            ])
            .arg(&opt)
            .output();
        if let Ok(o) = r {
            if o.status.success() {
                let _ = std::fs::rename(&opt, output_path);
            } else {
                let _ = std::fs::remove_file(&opt);
            }
        }
    }

    Ok(std::fs::metadata(output_path)?.len())
}

fn save_as_gif_ffmpeg(
    source: &Path,
    segments: &[(f64, f64, f64)],
    scale: u32,
    fps: Option<u32>,
    gifski: bool,
    out: &Path,
) -> anyhow::Result<u64> {
    let fps = fps.unwrap_or(15);
    let sf = if scale < 100 {
        format!(",scale=iw*{}/100:ih*{}/100", scale, scale)
    } else {
        String::new()
    };

    let any_speed_change = segments.iter().any(|&(_, _, s)| (s - 1.0).abs() >= 0.001);

    let input_source = if segments.len() == 1 && !any_speed_change {
        None
    } else {
        let temp_dir = tempfile::TempDir::new()?;
        let mut part_paths = Vec::new();
        for (i, &(start, end, speed)) in segments.iter().enumerate() {
            let part = temp_dir.path().join(format!("part{}.mp4", i));
            ffmpeg_export_segment(source, start, end - start, speed, &part)?;
            part_paths.push(part);
        }
        let list_file = temp_dir.path().join("concat.txt");
        let list_content: String = part_paths
            .iter()
            .map(|p| format!("file '{}'\n", p.display()))
            .collect();
        std::fs::write(&list_file, &list_content)?;

        let merged = temp_dir.path().join("merged.mp4");
        let o = std::process::Command::new("ffmpeg")
            .args(["-y", "-f", "concat", "-safe", "0", "-i"])
            .arg(&list_file)
            .args(["-c", "copy"])
            .arg(&merged)
            .output()?;
        if !o.status.success() {
            return Err(anyhow::anyhow!(
                "ffmpeg concat failed: {}",
                String::from_utf8_lossy(&o.stderr)
            ));
        }
        let merged_owned = merged.to_path_buf();
        std::mem::forget(temp_dir);
        Some(merged_owned)
    };

    let (actual_source, ss_arg, t_arg) = match &input_source {
        Some(merged) => (merged.as_path(), None, None),
        None => {
            let (start, end, _) = segments[0];
            (source, Some(start), Some(end - start))
        }
    };

    if gifski {
        let vf = format!("fps={}{}", fps, sf);
        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.arg("-y");
        if let Some(ss) = ss_arg {
            cmd.args(["-ss", &format!("{:.3}", ss)]);
        }
        if let Some(t) = t_arg {
            cmd.args(["-t", &format!("{:.3}", t)]);
        }
        let ff = cmd
            .args(["-i"])
            .arg(actual_source)
            .args(["-vf", &vf, "-f", "yuv4mpegpipe", "pipe:1"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let o = std::process::Command::new("gifski")
            .args(["--fps"])
            .arg(fps.to_string())
            .args(["-o"])
            .arg(out)
            .arg("-")
            .stdin(ff.stdout.unwrap())
            .output()?;
        if !o.status.success() {
            return Err(anyhow::anyhow!("gifski failed"));
        }
    } else {
        let vf = format!(
            "fps={}{},split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse=dither=bayer",
            fps, sf
        );
        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.arg("-y");
        if let Some(ss) = ss_arg {
            cmd.args(["-ss", &format!("{:.3}", ss)]);
        }
        if let Some(t) = t_arg {
            cmd.args(["-t", &format!("{:.3}", t)]);
        }
        let o = cmd
            .args(["-i"])
            .arg(actual_source)
            .args(["-vf", &vf])
            .arg(out)
            .output()?;
        if !o.status.success() {
            return Err(anyhow::anyhow!(
                "ffmpeg GIF export failed: {}",
                String::from_utf8_lossy(&o.stderr)
            ));
        }
    }

    if let Some(merged) = input_source {
        let _ = std::fs::remove_file(&merged);
        if let Some(parent) = merged.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }

    Ok(std::fs::metadata(out)?.len())
}

fn ffmpeg_export_segment(
    source: &Path,
    start: f64,
    dur: f64,
    speed: f64,
    out: &Path,
) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(["-y", "-ss"])
        .arg(format!("{:.3}", start))
        .args(["-t"])
        .arg(format!("{:.3}", dur))
        .args(["-i"])
        .arg(source);

    if (speed - 1.0).abs() < 0.001 {
        cmd.args(["-c", "copy"]);
    } else {
        let setpts = format!("setpts={}*PTS", 1.0 / speed);
        cmd.args(["-filter:v", &setpts, "-an"]);
    }

    let o = cmd.arg(out).output()?;
    if !o.status.success() {
        return Err(anyhow::anyhow!(
            "ffmpeg failed: {}",
            String::from_utf8_lossy(&o.stderr)
        ));
    }
    Ok(())
}

fn save_video_segments(
    source: &Path,
    segments: &[(f64, f64, f64)],
    out: &Path,
) -> anyhow::Result<u64> {
    let tmp = out.with_extension("tmp.mp4");

    if segments.len() == 1 {
        let (start, end, speed) = segments[0];
        ffmpeg_export_segment(source, start, end - start, speed, &tmp)?;
    } else {
        let temp_dir = tempfile::TempDir::new()?;
        let mut part_paths = Vec::new();
        for (i, &(start, end, speed)) in segments.iter().enumerate() {
            let part = temp_dir.path().join(format!("part{}.mp4", i));
            ffmpeg_export_segment(source, start, end - start, speed, &part)?;
            part_paths.push(part);
        }
        let list_file = temp_dir.path().join("concat.txt");
        let list_content: String = part_paths
            .iter()
            .map(|p| format!("file '{}'\n", p.display()))
            .collect();
        std::fs::write(&list_file, &list_content)?;

        let o = std::process::Command::new("ffmpeg")
            .args(["-y", "-f", "concat", "-safe", "0", "-i"])
            .arg(&list_file)
            .args(["-c", "copy"])
            .arg(&tmp)
            .output()?;
        if !o.status.success() {
            let _ = std::fs::remove_file(&tmp);
            return Err(anyhow::anyhow!(
                "ffmpeg concat failed: {}",
                String::from_utf8_lossy(&o.stderr)
            ));
        }
    }

    std::fs::rename(&tmp, out)?;
    Ok(std::fs::metadata(out)?.len())
}

// ── Color extraction ─────────────────────────────────────────────────────

fn extract_gif_colors(frames: &[RgbaImage]) -> Vec<[u8; 3]> {
    frames
        .iter()
        .map(|img| {
            let pixels = img.as_raw();
            let count = (pixels.len() / 4) as u64;
            if count == 0 {
                return [0, 0, 0];
            }
            let (mut r_sum, mut g_sum, mut b_sum) = (0u64, 0u64, 0u64);
            for chunk in pixels.chunks_exact(4) {
                r_sum += chunk[0] as u64;
                g_sum += chunk[1] as u64;
                b_sum += chunk[2] as u64;
            }
            [
                (r_sum / count) as u8,
                (g_sum / count) as u8,
                (b_sum / count) as u8,
            ]
        })
        .collect()
}

fn extract_video_colors(path: &Path, duration: f64, num_samples: usize) -> Vec<[u8; 3]> {
    if duration <= 0.0 || num_samples == 0 {
        return Vec::new();
    }
    let fps = num_samples as f64 / duration;
    let output = std::process::Command::new("ffmpeg")
        .args(["-i"])
        .arg(path)
        .args([
            "-vf",
            &format!("fps={:.4},scale=1:1", fps),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();

    match output {
        Ok(o) if o.status.success() => o
            .stdout
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect(),
        _ => Vec::new(),
    }
}

// ── MediaEditor impl ──────────────────────────────────────────────────────

impl MediaEditor {
    fn speed_at(&self, time: f64) -> f64 {
        let chunk = match self.cuts.iter().position(|&c| time < c) {
            Some(i) => i,
            None => self.cuts.len(),
        };
        self.chunk_speeds.get(&chunk).copied().unwrap_or(1.0)
    }

    fn original_fps(&self) -> f64 {
        match &self.media {
            MediaState::Gif { delay_ms, .. } => 1000.0 / (*delay_ms).max(1) as f64,
            MediaState::VideoLoaded { video } => video.framerate(),
            _ => 15.0,
        }
    }

    /// For GIF mode: map position (seconds) to frame index.
    fn frame_at_seconds(&self, secs: f64) -> usize {
        if let MediaState::Gif { frames, .. } = &self.media {
            if frames.is_empty() || self.duration <= 0.0 {
                return 0;
            }
            let frac = secs / self.duration;
            ((frac * frames.len() as f64) as usize).min(frames.len() - 1)
        } else {
            0
        }
    }

    fn is_in_deleted_chunk(&self, time: f64) -> bool {
        Self::time_in_deleted(time, &self.cuts, &self.deleted_chunks)
    }

    fn next_non_deleted_time(&self, time: f64) -> Option<f64> {
        Self::find_next_non_deleted(time, &self.cuts, &self.deleted_chunks)
    }

    fn first_non_deleted_time(&self) -> f64 {
        Self::find_first_non_deleted(&self.cuts, &self.deleted_chunks)
    }

    fn time_in_deleted(time: f64, cuts: &[f64], deleted: &HashSet<usize>) -> bool {
        let chunk = match cuts.iter().position(|&c| time < c) {
            Some(i) => i,
            None => cuts.len(),
        };
        deleted.contains(&chunk)
    }

    fn find_next_non_deleted(time: f64, cuts: &[f64], deleted: &HashSet<usize>) -> Option<f64> {
        let num_chunks = cuts.len() + 1;
        let chunk = match cuts.iter().position(|&c| time < c) {
            Some(i) => i,
            None => cuts.len(),
        };
        for i in chunk..num_chunks {
            if !deleted.contains(&i) {
                let start = if i == 0 { 0.0 } else { cuts[i - 1] };
                return Some(start.max(time));
            }
        }
        None
    }

    fn find_first_non_deleted(cuts: &[f64], deleted: &HashSet<usize>) -> f64 {
        let num_chunks = cuts.len() + 1;
        for i in 0..num_chunks {
            if !deleted.contains(&i) {
                return if i == 0 { 0.0 } else { cuts[i - 1] };
            }
        }
        0.0
    }

    fn current_frame_index(&self) -> usize {
        self.frame_at_seconds(self.position)
    }

    fn kept_segments(&self) -> Vec<(f64, f64, f64)> {
        let num_chunks = self.cuts.len() + 1;
        let mut segments = Vec::new();
        for i in 0..num_chunks {
            if self.deleted_chunks.contains(&i) {
                continue;
            }
            let start = if i == 0 {
                self.trim_start
            } else {
                self.cuts[i - 1].max(self.trim_start)
            };
            let end = if i < self.cuts.len() {
                self.cuts[i].min(self.trim_end)
            } else {
                self.trim_end
            };
            let speed = self.chunk_speeds.get(&i).copied().unwrap_or(1.0);
            if end > start {
                segments.push((start, end, speed));
            }
        }
        if segments.is_empty() {
            segments.push((self.trim_start, self.trim_end, 1.0));
        }
        segments
    }

    fn export_to(&self, output_path: &Path) -> anyhow::Result<u64> {
        let segments = self.kept_segments();
        match self.export_format {
            ExportFormat::Gif => {
                if self.ffmpeg_available && self.media_type == MediaType::Video {
                    save_as_gif_ffmpeg(
                        &self.media_path,
                        &segments,
                        self.output_scale,
                        self.output_fps,
                        self.gifski_available,
                        output_path,
                    )
                } else if let MediaState::Gif {
                    frames, delay_ms, ..
                } = &self.media
                {
                    save_trimmed_gif(
                        frames,
                        *delay_ms,
                        &segments,
                        self.duration,
                        self.output_scale,
                        self.output_fps,
                        self.use_ffmpeg,
                        output_path,
                    )
                } else {
                    Err(anyhow::anyhow!("No frames to export"))
                }
            }
            ExportFormat::Video => {
                if self.ffmpeg_available {
                    save_video_segments(&self.media_path, &segments, output_path)
                } else {
                    Err(anyhow::anyhow!("ffmpeg required for video export"))
                }
            }
        }
    }
}

// ── Application ───────────────────────────────────────────────────────────

impl Application for MediaEditor {
    type Executor = executor::Default;
    type Flags = Flags;
    type Message = Message;

    const APP_ID: &'static str = "io.github.hojjatabdollahi.snappea.edit";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(core: cosmic::Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let temp_dir = tempfile::TempDir::new().expect("failed to create temp dir");

        let ffmpeg_version = std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.lines().next().map(|l| l.to_string()));
        let ffmpeg_available = ffmpeg_version.is_some();

        let gifski_version = std::process::Command::new("gifski")
            .arg("--version")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                let out = String::from_utf8(o.stdout).unwrap_or_default();
                let err = String::from_utf8(o.stderr).unwrap_or_default();
                let s = if out.trim().is_empty() { err } else { out };
                Some(s.trim().to_string())
            })
            .filter(|s| !s.is_empty());
        let gifski_available = gifski_version.is_some();

        let file_size_bytes = std::fs::metadata(&flags.media_path)
            .map(|m| m.len())
            .unwrap_or(0);

        // Try loading based on media type
        let (media, duration, loaded, status, task, init_colors) = match flags.media_type {
            MediaType::Video => {
                if flags.media_path.exists() {
                    let uri = url::Url::from_file_path(&flags.media_path).unwrap();
                    match Video::new(&uri) {
                        Ok(mut video) => {
                            video.set_paused(true);
                            let dur = video.duration().as_secs_f64();
                            let (w, h) = video.size();
                            let status = format!("{:.1}s, {}x{}", dur, w, h);
                            let color_path = flags.media_path.clone();
                            (
                                MediaState::VideoLoaded { video },
                                dur,
                                true,
                                status,
                                Task::perform(
                                    async move {
                                        tokio::task::spawn_blocking(move || {
                                            extract_video_colors(&color_path, dur, 300)
                                        })
                                        .await
                                        .unwrap_or_default()
                                    },
                                    |colors| cosmic::Action::App(Message::ColorsExtracted(colors)),
                                ),
                                Vec::new(),
                            )
                        }
                        Err(e) => {
                            log::error!("Video load failed: {}", e);
                            (
                                MediaState::Loading,
                                0.0,
                                false,
                                fl!("edit-loading-video"),
                                Task::perform(
                                    async { tokio::time::sleep(Duration::from_millis(500)).await },
                                    |_| cosmic::Action::App(Message::PollFile),
                                ),
                                Vec::new(),
                            )
                        }
                    }
                } else {
                    (
                        MediaState::Loading,
                        0.0,
                        false,
                        fl!("edit-loading-video"),
                        Task::perform(
                            async { tokio::time::sleep(Duration::from_millis(500)).await },
                            |_| cosmic::Action::App(Message::PollFile),
                        ),
                        Vec::new(),
                    )
                }
            }
            MediaType::Gif => {
                if flags.media_path.exists() {
                    match load_gif(&flags.media_path) {
                        Ok((frames, delay_ms)) => {
                            let dur = frames.len() as f64 * delay_ms as f64 / 1000.0;
                            let status = format!(
                                "{} frames, {:.1}s, {}x{}",
                                frames.len(),
                                dur,
                                frames.first().map_or(0, |f| f.width()),
                                frames.first().map_or(0, |f| f.height()),
                            );
                            let colors = extract_gif_colors(&frames);
                            let gif_frames = build_gif_frames(&frames, delay_ms);
                            (
                                MediaState::Gif {
                                    frames,
                                    gif_frames,
                                    delay_ms,
                                },
                                dur,
                                true,
                                status,
                                Task::none(),
                                colors,
                            )
                        }
                        Err(e) => {
                            log::error!("GIF load failed: {}", e);
                            (
                                MediaState::Loading,
                                0.0,
                                false,
                                fl!("edit-encoding-gif"),
                                Task::perform(
                                    async { tokio::time::sleep(Duration::from_millis(200)).await },
                                    |_| cosmic::Action::App(Message::PollFile),
                                ),
                                Vec::new(),
                            )
                        }
                    }
                } else {
                    (
                        MediaState::Loading,
                        0.0,
                        false,
                        fl!("edit-encoding-gif"),
                        Task::perform(
                            async { tokio::time::sleep(Duration::from_millis(200)).await },
                            |_| cosmic::Action::App(Message::PollFile),
                        ),
                        Vec::new(),
                    )
                }
            }
        };

        (
            Self {
                core,
                media_path: flags.media_path,
                media_type: flags.media_type,
                media,
                _temp_dir: temp_dir,
                duration,
                position: 0.0,
                trim_start: 0.0,
                trim_end: duration,
                playing: false,
                dragging: false,
                cuts: Vec::new(),
                selected_chunk: None,
                deleted_chunks: HashSet::new(),
                chunk_speeds: HashMap::new(),
                frame_colors: init_colors,
                loaded,
                poll_count: 0,
                status,
                can_discard: flags.can_discard,
                export_format: match flags.media_type {
                    MediaType::Gif => ExportFormat::Gif,
                    MediaType::Video => ExportFormat::Video,
                },
                output_scale: 100,
                output_fps: None,
                ffmpeg_available,
                ffmpeg_version,
                gifski_available,
                gifski_version,
                use_ffmpeg: false,
                file_size_bytes,
            },
            task,
        )
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            // ── Timeline ──────────────────────────────────────────────
            Message::Seek(pos) => {
                let clamped = pos.clamp(self.trim_start, self.trim_end);
                self.position = clamped;
                self.dragging = true;
                if let MediaState::VideoLoaded { video } = &mut self.media {
                    video.set_paused(true);
                    let _ = video.seek(Duration::from_secs_f64(clamped), false);
                } else {
                    self.playing = false;
                }
            }
            Message::SeekRelease => {
                self.dragging = false;
                self.position = self.position.clamp(self.trim_start, self.trim_end);
                if let MediaState::VideoLoaded { video } = &mut self.media {
                    let _ = video.seek(Duration::from_secs_f64(self.position), true);
                    if self.playing {
                        video.set_paused(false);
                    }
                }
            }
            Message::TrimChanged((s, e)) => {
                self.trim_start = s;
                self.trim_end = e;
                if self.position < s || self.position > e {
                    self.position = s;
                    if let MediaState::VideoLoaded { video } = &mut self.media {
                        let _ = video.seek(Duration::from_secs_f64(s), false);
                        if !self.playing {
                            video.set_paused(true);
                        }
                    }
                }
            }

            // ── Cut editing ──────────────────────────────────────────
            Message::CutAtPlayhead => {
                let pos = self.position;
                if pos > 0.0 && pos < self.duration {
                    if !self.cuts.iter().any(|&c| (c - pos).abs() < 0.01) {
                        self.cuts.push(pos);
                        self.cuts.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    }
                }
            }
            Message::SelectChunk(chunk) => {
                self.selected_chunk = chunk;
            }
            Message::DeleteSelectedChunk => {
                if let Some(chunk) = self.selected_chunk {
                    if self.deleted_chunks.contains(&chunk) {
                        self.deleted_chunks.remove(&chunk);
                    } else {
                        self.deleted_chunks.insert(chunk);
                    }
                }
            }
            Message::SetChunkSpeed(speed_idx) => {
                if let Some(chunk) = self.selected_chunk {
                    let speed = SPEED_OPTIONS[speed_idx.min(SPEED_OPTIONS.len() - 1)];
                    if (speed - 1.0).abs() < f64::EPSILON {
                        self.chunk_speeds.remove(&chunk);
                    } else {
                        self.chunk_speeds.insert(chunk, speed);
                    }
                }
            }
            Message::MoveCut(index, new_time) => {
                if index < self.cuts.len() {
                    self.cuts[index] = new_time;
                    self.cuts.sort_by(|a, b| a.partial_cmp(b).unwrap());
                }
            }
            Message::AddCut(time) => {
                if time > 0.0 && time < self.duration {
                    if !self.cuts.iter().any(|&c| (c - time).abs() < 0.01) {
                        self.cuts.push(time);
                        self.cuts.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    }
                }
            }
            Message::RemoveCut(index) => {
                if index < self.cuts.len() {
                    self.cuts.remove(index);
                    self.deleted_chunks.clear();
                    self.selected_chunk = None;
                    self.chunk_speeds.clear();
                }
            }
            Message::ColorsExtracted(colors) => {
                self.frame_colors = colors;
            }

            // ── Playback ──────────────────────────────────────────────
            Message::TogglePlay => {
                self.playing = !self.playing;
                let in_deleted =
                    Self::time_in_deleted(self.position, &self.cuts, &self.deleted_chunks);
                let fallback =
                    Self::find_next_non_deleted(self.trim_start, &self.cuts, &self.deleted_chunks)
                        .unwrap_or(self.trim_start);
                let start_pos = if self.playing
                    && (self.position < self.trim_start
                        || self.position >= self.trim_end
                        || in_deleted)
                {
                    fallback
                } else {
                    self.position
                };
                let speed = self.speed_at(start_pos);
                match &mut self.media {
                    MediaState::VideoLoaded { video } => {
                        if self.playing {
                            self.position = start_pos;
                            video.set_paused(true);
                            let _ = video.set_speed(speed);
                            let _ = video.seek(Duration::from_secs_f64(start_pos), true);
                            video.set_paused(false);
                        } else {
                            video.set_paused(true);
                        }
                    }
                    MediaState::Gif { .. } => {
                        if self.playing {
                            self.position = fallback;
                        }
                    }
                    _ => {}
                }
            }
            Message::NewFrame => {
                if !self.dragging {
                    if let MediaState::VideoLoaded { video } = &mut self.media {
                        let pos = video.position().as_secs_f64();
                        let in_deleted =
                            Self::time_in_deleted(pos, &self.cuts, &self.deleted_chunks);
                        let loop_start =
                            Self::find_first_non_deleted(&self.cuts, &self.deleted_chunks)
                                .max(self.trim_start);
                        let next_non_del =
                            Self::find_next_non_deleted(pos, &self.cuts, &self.deleted_chunks);

                        let chunk_at = |t: f64| -> usize {
                            match self.cuts.iter().position(|&c| t < c) {
                                Some(i) => i,
                                None => self.cuts.len(),
                            }
                        };
                        let speed_for = |t: f64| -> f64 {
                            self.chunk_speeds.get(&chunk_at(t)).copied().unwrap_or(1.0)
                        };

                        if self.playing && pos >= self.trim_end {
                            let _ = video.set_speed(speed_for(loop_start));
                            let _ = video.seek(Duration::from_secs_f64(loop_start), true);
                            self.position = loop_start;
                        } else if self.playing && pos < self.trim_start {
                            let _ = video.seek(Duration::from_secs_f64(self.trim_start), true);
                            self.position = self.trim_start;
                        } else if self.playing && in_deleted {
                            let target = match next_non_del {
                                Some(t) if t <= self.trim_end => t,
                                _ => loop_start,
                            };
                            let _ = video.set_speed(speed_for(target));
                            let _ = video.seek(Duration::from_secs_f64(target), true);
                            self.position = target;
                        } else if self.playing {
                            let desired = speed_for(pos);
                            if (desired - video.speed()).abs() > 0.001 {
                                let _ = video.set_speed(desired);
                            }
                            self.position = pos;
                        } else {
                            self.position = pos;
                        }
                    }
                }
            }
            Message::GifFrameChanged(index) => {
                if let MediaState::Gif { frames, .. } = &self.media {
                    if !frames.is_empty() && self.duration > 0.0 {
                        let pos = index as f64 / frames.len() as f64 * self.duration;
                        if self.playing && self.is_in_deleted_chunk(pos) {
                            if let Some(next) = self.next_non_deleted_time(pos) {
                                if next <= self.trim_end {
                                    self.position = next;
                                } else {
                                    self.position = self.first_non_deleted_time();
                                }
                            } else {
                                self.position = self.first_non_deleted_time();
                            }
                        } else {
                            self.position = pos;
                        }
                    }
                }
            }
            Message::DurationChanged(dur) => {
                let d = dur.as_secs_f64();
                if d > 0.0 {
                    self.duration = d;
                    if self.trim_end <= 0.0 || self.trim_end > d {
                        self.trim_end = d;
                    }
                }
            }
            Message::EndOfStream => {
                if self.playing {
                    if let MediaState::VideoLoaded { video } = &mut self.media {
                        let _ = video.seek(Duration::from_secs_f64(self.trim_start), false);
                    }
                }
            }

            // ── Loading ───────────────────────────────────────────────
            Message::PollFile => {
                if self.loaded {
                    return Task::none();
                }
                self.poll_count += 1;
                let path = self.media_path.clone();
                let media_type = self.media_type;
                return Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        if !path.exists() {
                            return Err("not ready".to_string());
                        }
                        match media_type {
                            MediaType::Video => Ok(None),
                            MediaType::Gif => match load_gif(&path) {
                                Ok((frames, delay_ms)) => {
                                    let data: Vec<(u32, u32, Vec<u8>)> = frames
                                        .iter()
                                        .map(|f| (f.width(), f.height(), f.as_raw().clone()))
                                        .collect();
                                    let dur = frames.len() as f64 * delay_ms as f64 / 1000.0;
                                    Ok(Some((data, delay_ms, dur)))
                                }
                                Err(_) => Err("not ready".to_string()),
                            },
                        }
                    },
                    |result| match result {
                        Ok(Some((data, delay_ms, dur))) => {
                            cosmic::Action::App(Message::GifLoaded(data, delay_ms, dur))
                        }
                        Ok(None) => cosmic::Action::App(Message::VideoReady),
                        Err(_) => cosmic::Action::App(Message::PollFile),
                    },
                );
            }
            Message::GifLoaded(data, delay_ms, duration) => {
                let frames: Vec<RgbaImage> = data
                    .iter()
                    .map(|(w, h, px)| {
                        RgbaImage::from_raw(*w, *h, px.clone())
                            .unwrap_or_else(|| RgbaImage::new(1, 1))
                    })
                    .collect();
                self.frame_colors = extract_gif_colors(&frames);
                let gif_frames = build_gif_frames(&frames, delay_ms);
                self.duration = duration;
                self.trim_end = duration;
                self.media = MediaState::Gif {
                    frames,
                    gif_frames,
                    delay_ms,
                };
                self.loaded = true;
                self.file_size_bytes = std::fs::metadata(&self.media_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                self.status = fl!("edit-loaded");
            }
            Message::VideoReady => {
                let uri = url::Url::from_file_path(&self.media_path).unwrap();
                match Video::new(&uri) {
                    Ok(mut video) => {
                        video.set_paused(true);
                        let dur = video.duration().as_secs_f64();
                        let (w, h) = video.size();
                        self.duration = dur;
                        self.trim_end = dur;
                        self.media = MediaState::VideoLoaded { video };
                        self.loaded = true;
                        self.file_size_bytes = std::fs::metadata(&self.media_path)
                            .map(|m| m.len())
                            .unwrap_or(0);
                        self.status = format!("{:.1}s, {}x{}", dur, w, h);

                        let path = self.media_path.clone();
                        return Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    extract_video_colors(&path, dur, 300)
                                })
                                .await
                                .unwrap_or_default()
                            },
                            |colors| cosmic::Action::App(Message::ColorsExtracted(colors)),
                        );
                    }
                    Err(_) => {
                        return Task::perform(
                            async { tokio::time::sleep(Duration::from_millis(500)).await },
                            |_| cosmic::Action::App(Message::PollFile),
                        );
                    }
                }
            }

            // ── Output settings ───────────────────────────────────────
            Message::SetExportFormat(i) => {
                self.export_format = match i {
                    0 => ExportFormat::Gif,
                    _ => ExportFormat::Video,
                };
            }
            Message::SetOutputScale(i) => {
                self.output_scale = match i {
                    0 => 100,
                    1 => 75,
                    _ => 50,
                };
            }
            Message::SetOutputFps(i) => {
                self.output_fps = match i {
                    0 => None,
                    1 => Some(10),
                    _ => Some(5),
                };
            }
            Message::SetUseFfmpeg(v) => {
                self.use_ffmpeg = v;
            }

            // ── Actions ───────────────────────────────────────────────
            Message::Save => {
                let path = self.media_path.clone();
                match self.export_to(&path) {
                    Ok(size) => {
                        self.status = fl!(
                            "edit-saved-size",
                            size = format!("{:.1}", size as f64 / 1024.0)
                        )
                    }
                    Err(e) => self.status = fl!("edit-save-failed", error = e.to_string()),
                }
            }
            Message::SaveAs => {
                let stem = self
                    .media_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let ext = match self.export_format {
                    ExportFormat::Gif => "gif",
                    ExportFormat::Video => self
                        .media_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("mp4"),
                };
                let default_name = fl!("edit-trimmed-filename", stem = stem, ext = ext);
                let start_dir = self.media_path.parent().map(|p| p.to_path_buf());
                return Task::perform(
                    async move {
                        let mut d = rfd::AsyncFileDialog::new().set_file_name(&default_name);
                        if let Some(dir) = start_dir {
                            d = d.set_directory(dir);
                        }
                        d.save_file().await.map(|f| f.path().to_path_buf())
                    },
                    |p| cosmic::Action::App(Message::SaveAsChosen(p)),
                );
            }
            Message::SaveAsChosen(Some(path)) => match self.export_to(&path) {
                Ok(size) => {
                    self.status = fl!(
                        "edit-saved-path-size",
                        path = path.display().to_string(),
                        size = format!("{:.1}", size as f64 / 1024.0)
                    )
                }
                Err(e) => self.status = fl!("edit-save-failed", error = e.to_string()),
            },
            Message::SaveAsChosen(None) => {}
            Message::CopyToClipboard => {
                let ext = match self.export_format {
                    ExportFormat::Gif => "gif",
                    ExportFormat::Video => self
                        .media_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("mp4"),
                };
                let tmp_path = PathBuf::from(format!("/tmp/snappea-clipboard.{}", ext));

                match self.export_to(&tmp_path) {
                    Ok(_) => {
                        if self.export_format == ExportFormat::Gif {
                            // GIF: put image data on clipboard
                            let gif_bytes = std::fs::read(&tmp_path).unwrap_or_default();
                            let png_bytes = match &self.media {
                                MediaState::Gif { frames, .. } => frames
                                    .get(self.current_frame_index())
                                    .and_then(|img| {
                                        let mut buf = Vec::new();
                                        img.write_to(
                                            &mut std::io::Cursor::new(&mut buf),
                                            image::ImageFormat::Png,
                                        )
                                        .ok()?;
                                        Some(buf)
                                    })
                                    .unwrap_or_default(),
                                _ => Vec::new(),
                            };
                            let uri = format!("file://{}", tmp_path.display());
                            self.status = fl!("edit-copied-to-clipboard");
                            return cosmic::iced::runtime::clipboard::write_data(ClipboardData {
                                png_bytes,
                                gif_bytes,
                                file_uri: uri,
                            })
                            .map(|_: ()| cosmic::Action::App(Message::CopyDone(Ok(()))));
                        } else {
                            // Video: copy file URI to clipboard
                            let uri = format!("file://{}", tmp_path.display());
                            self.status = fl!("edit-copied-file-path");
                            return cosmic::iced::runtime::clipboard::write_data(ClipboardData {
                                png_bytes: Vec::new(),
                                gif_bytes: Vec::new(),
                                file_uri: uri,
                            })
                            .map(|_: ()| cosmic::Action::App(Message::CopyDone(Ok(()))));
                        }
                    }
                    Err(e) => {
                        self.status = fl!("edit-copy-failed", error = e.to_string());
                    }
                }
            }
            Message::CopyDone(r) => {
                self.status = match r {
                    Ok(()) => fl!("edit-copied"),
                    Err(e) => fl!("edit-copy-failed", error = e),
                };
            }
            Message::Discard => {
                let _ = std::fs::remove_file(&self.media_path);
                std::process::exit(0);
            }
            Message::ToggleAbout => {
                let show = !self.core().window.show_context;
                self.core_mut().set_show_context(show);
            }
        }
        Task::none()
    }

    fn header_end(&self) -> Vec<cosmic::Element<'_, Self::Message>> {
        vec![
            widget::button::custom(icon::from_name("help-about-symbolic").size(20).icon())
                .class(cosmic::theme::Button::Icon)
                .on_press(Message::ToggleAbout)
                .into(),
        ]
    }

    fn context_drawer(
        &self,
    ) -> Option<cosmic::app::context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core().window.show_context {
            return None;
        }

        let git_hash = env!("GIT_HASH");
        let version = env!("CARGO_PKG_VERSION");

        let mut items: Vec<cosmic::Element<'_, Message>> = vec![
            widget::container(
                icon::from_name("io.github.hojjatabdollahi.snappea")
                    .size(64)
                    .icon(),
            )
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .into(),
            widget::text::title3(fl!("edit-title")).into(),
            widget::text::caption(fl!("app-version", version = version, hash = git_hash)).into(),
            cosmic::widget::divider::horizontal::light().into(),
        ];

        items.push(widget::text::title4(fl!("edit-system-tools")).into());

        if let Some(ver) = &self.ffmpeg_version {
            items.push(
                widget::text::body(fl!("edit-ffmpeg-version", version = ver.as_str())).into(),
            );
        } else {
            items.push(widget::text::body(fl!("edit-ffmpeg-not-installed")).into());
        }

        if let Some(ver) = &self.gifski_version {
            items.push(
                widget::text::body(fl!("edit-gifski-version", version = ver.as_str())).into(),
            );
        } else {
            items.push(widget::text::body(fl!("edit-gifski-not-installed")).into());
        }

        let content = widget::column::with_children(items)
            .spacing(12)
            .padding(16)
            .align_x(Alignment::Center);

        Some(
            cosmic::app::context_drawer::context_drawer(content, Message::ToggleAbout)
                .title(fl!("edit-about")),
        )
    }

    fn view(&self) -> cosmic::Element<'_, Self::Message> {
        // Loading screen
        if !self.loaded {
            let dots = ".".repeat((self.poll_count % 4) + 1);
            return widget::container(
                widget::column::with_children(vec![
                    widget::text::title3(format!("{}{}", fl!("edit-loading"), dots)).into(),
                    widget::text::caption(format!("{}", self.media_path.display())).into(),
                ])
                .spacing(12)
                .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into();
        }

        // ── Player widget ─────────────────────────────────────────
        let player_widget: cosmic::Element<'_, Message> = match &self.media {
            MediaState::VideoLoaded { video } => widget::container(
                VideoPlayer::new(video)
                    .on_new_frame(Message::NewFrame)
                    .on_end_of_stream(Message::EndOfStream)
                    .on_duration_changed(|d| Message::DurationChanged(d))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Contain),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into(),
            MediaState::Gif { gif_frames, .. } => {
                let trim_s = self.frame_at_seconds(self.trim_start);
                let trim_e = self.frame_at_seconds(self.trim_end);
                let mut player = gif_player::gif_player(gif_frames)
                    .playing(self.playing)
                    .trim(trim_s, trim_e)
                    .on_frame(Message::GifFrameChanged)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Contain);
                if !self.playing {
                    player = player.seek(self.current_frame_index());
                }
                widget::container(player)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::Center)
                    .into()
            }
            _ => widget::text::body(fl!("edit-no-media")).into(),
        };

        // Play/pause overlay
        let preview: cosmic::Element<'_, Message> = if self.playing {
            cosmic::widget::mouse_area(player_widget)
                .on_press(Message::TogglePlay)
                .into()
        } else {
            let overlay: cosmic::Element<'_, Message> = widget::container(
                widget::container(
                    icon::from_name("media-playback-start-symbolic")
                        .size(48)
                        .icon(),
                )
                .width(Length::Fixed(112.0))
                .height(Length::Fixed(112.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .class(cosmic::theme::Container::Custom(Box::new(|_| {
                    cosmic::iced::widget::container::Style {
                        background: Some(cosmic::iced::Background::Color(
                            cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45),
                        )),
                        text_color: Some(cosmic::iced::Color::from_rgba(1.0, 1.0, 1.0, 0.9)),
                        border: cosmic::iced::Border {
                            radius: 56.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }))),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into();

            let stacked: cosmic::Element<'_, Message> =
                cosmic::iced::widget::stack![player_widget, overlay]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            cosmic::widget::mouse_area(stacked)
                .on_press(Message::TogglePlay)
                .into()
        };

        // ── Scrubber ──────────────────────────────────────────────
        let scrub = widget::container(
            video_scrubber(self.duration, self.position)
                .colors(&self.frame_colors)
                .cuts(&self.cuts)
                .deleted_chunks(&self.deleted_chunks)
                .selected_chunk(self.selected_chunk)
                .on_seek(Message::Seek)
                .on_release(Message::SeekRelease)
                .on_select_chunk(Message::SelectChunk)
                .on_cut_moved(Message::MoveCut)
                .on_cut_added(Message::AddCut)
                .on_cut_removed(Message::RemoveCut),
        )
        .padding([0, 8]);

        // ── Cut toolbar ──────────────────────────────────────────
        let btn_cut = widget::button::custom(
            widget::row::with_children(vec![
                icon::from_name("edit-cut-symbolic").size(16).icon().into(),
                widget::text::body(fl!("edit-cut")).into(),
            ])
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .class(cosmic::theme::Button::Standard)
        .on_press(Message::CutAtPlayhead);

        let selected_is_deleted = self
            .selected_chunk
            .is_some_and(|idx| self.deleted_chunks.contains(&idx));

        let btn_delete_chunk = if self.selected_chunk.is_some() {
            let (icon_name, label) = if selected_is_deleted {
                ("edit-undo-symbolic", fl!("edit-undelete-chunk"))
            } else {
                ("edit-delete-symbolic", fl!("edit-delete-chunk"))
            };
            widget::button::custom(
                widget::row::with_children(vec![
                    icon::from_name(icon_name).size(16).icon().into(),
                    widget::text::body(label).into(),
                ])
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .class(if selected_is_deleted {
                cosmic::theme::Button::Standard
            } else {
                cosmic::theme::Button::Destructive
            })
            .on_press(Message::DeleteSelectedChunk)
        } else {
            widget::button::custom(
                widget::row::with_children(vec![
                    icon::from_name("edit-delete-symbolic")
                        .size(16)
                        .icon()
                        .into(),
                    widget::text::body(fl!("edit-delete-chunk")).into(),
                ])
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .class(cosmic::theme::Button::Standard)
        };

        let mut cut_items: Vec<cosmic::Element<'_, Message>> =
            vec![btn_cut.into(), btn_delete_chunk.into()];

        if let Some(sel) = self.selected_chunk {
            let current_speed = self.chunk_speeds.get(&sel).copied().unwrap_or(1.0);
            let active_idx = SPEED_OPTIONS
                .iter()
                .position(|&s| (s - current_speed).abs() < 0.001)
                .unwrap_or(3);

            cut_items.push(
                widget::row::with_children(vec![
                    icon::from_name("media-playback-speed-symbolic")
                        .size(16)
                        .icon()
                        .into(),
                    widget::text::caption(fl!("edit-speed")).into(),
                    Toggle::with_labels(SPEED_LABELS, active_idx)
                        .on_select(Message::SetChunkSpeed)
                        .pill_thickness(26.0)
                        .circle_size(22.0)
                        .into(),
                ])
                .spacing(4)
                .align_y(Alignment::Center)
                .into(),
            );
        }

        let cut_toolbar = cosmic::widget::flex_row::flex_row(cut_items)
            .row_spacing(8)
            .column_spacing(8)
            .align_items(Alignment::Center)
            .width(Length::Fill);

        // ── Info ──────────────────────────────────────────────────
        let chunk_info = if let Some(sel) = self.selected_chunk {
            let deleted = if self.deleted_chunks.contains(&sel) {
                format!(" [{}]", fl!("edit-deleted"))
            } else {
                String::new()
            };
            format!("  |  {} {}{}", fl!("edit-chunk"), sel + 1, deleted)
        } else {
            String::new()
        };
        let info = widget::text::caption(format!(
            "{:.2}s / {:.2}s  |  {} {}{}",
            self.position,
            self.duration,
            fl!("edit-cuts"),
            self.cuts.len(),
            chunk_info,
        ));

        // ── Action buttons ────────────────────────────────────────
        let btn_save = widget::button::custom(
            widget::row::with_children(vec![
                icon::from_name("document-save-symbolic")
                    .size(16)
                    .icon()
                    .into(),
                widget::text::body(fl!("edit-save")).into(),
            ])
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .class(cosmic::theme::Button::Suggested)
        .on_press(Message::Save);

        let btn_save_as = widget::button::standard(fl!("edit-save-as")).on_press(Message::SaveAs);
        let btn_copy =
            widget::button::custom(icon::from_name("edit-copy-symbolic").size(20).icon())
                .class(cosmic::theme::Button::Icon)
                .on_press(Message::CopyToClipboard);

        let mut actions: Vec<cosmic::Element<'_, Message>> = Vec::new();
        if self.can_discard {
            actions.push(
                widget::button::destructive(fl!("edit-discard"))
                    .on_press(Message::Discard)
                    .into(),
            );
        }
        actions.push(cosmic::iced::widget::space().width(Length::Fill).into());
        actions.push(btn_copy.into());
        actions.push(btn_save_as.into());
        actions.push(btn_save.into());

        let action_row = widget::row::with_children(actions)
            .spacing(8)
            .align_y(Alignment::Center);
        let status_bar = widget::text::caption(&self.status);

        // ── Output settings ───────────────────────────────────────
        let format_gif_label = fl!("edit-format-gif");
        let format_video_label = fl!("edit-format-video");
        let format_section: cosmic::Element<'_, Message> = widget::row::with_children(vec![
            widget::text::caption(fl!("edit-format")).into(),
            Toggle::with_labels(
                &[&format_gif_label, &format_video_label],
                match self.export_format {
                    ExportFormat::Gif => 0,
                    _ => 1,
                },
            )
            .on_select(Message::SetExportFormat)
            .pill_thickness(26.0)
            .circle_size(22.0)
            .into(),
        ])
        .spacing(8)
        .align_y(Alignment::Center)
        .into();

        let original_fps_label = format!("{:.0}", self.original_fps());
        let scale_section: cosmic::Element<'_, Message> = widget::row::with_children(vec![
            widget::text::caption(fl!("edit-scale")).into(),
            Toggle::with_labels(
                &["100%", "75%", "50%"],
                match self.output_scale {
                    100 => 0,
                    75 => 1,
                    _ => 2,
                },
            )
            .on_select(Message::SetOutputScale)
            .pill_thickness(26.0)
            .circle_size(22.0)
            .into(),
        ])
        .spacing(8)
        .align_y(Alignment::Center)
        .into();

        let fps_section: cosmic::Element<'_, Message> = widget::row::with_children(vec![
            widget::text::caption("FPS").into(),
            Toggle::with_labels(
                &[&original_fps_label, "10", "5"],
                match self.output_fps {
                    None => 0,
                    Some(10) => 1,
                    _ => 2,
                },
            )
            .on_select(Message::SetOutputFps)
            .pill_thickness(26.0)
            .circle_size(22.0)
            .into(),
        ])
        .spacing(8)
        .align_y(Alignment::Center)
        .into();

        let size_label = if self.file_size_bytes > 0 {
            format!("{:.1} MB", self.file_size_bytes as f64 / 1_048_576.0)
        } else {
            String::new()
        };

        let mut output_items: Vec<cosmic::Element<'_, Message>> =
            vec![widget::text::caption(size_label).into(), format_section];
        if self.export_format == ExportFormat::Gif {
            output_items.push(scale_section);
            output_items.push(fps_section);
            if self.ffmpeg_available && self.media_type == MediaType::Gif {
                output_items.push(
                    widget::checkbox(self.use_ffmpeg)
                        .label(fl!("edit-optimize-ffmpeg"))
                        .on_toggle(Message::SetUseFfmpeg)
                        .into(),
                );
            }
        }
        let output_settings = cosmic::widget::flex_row::flex_row(output_items)
            .row_spacing(8)
            .column_spacing(16)
            .align_items(Alignment::Center)
            .width(Length::Fill);

        // ── About panel (collapsible) ─────────────────────────────
        widget::column::with_children(vec![
            preview.into(),
            scrub.into(),
            cut_toolbar.into(),
            info.into(),
            output_settings.into(),
            cosmic::widget::divider::horizontal::light().into(),
            action_row.into(),
            status_bar.into(),
        ])
        .spacing(8)
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
