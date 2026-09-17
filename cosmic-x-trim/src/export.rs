// SPDX-License-Identifier: GPL-3.0-only

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};

/// Output container. The codec follows from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Mp4,
    Mkv,
    Webm,
    Gif,
}

impl Format {
    pub const ALL: [Self; 4] = [Self::Mp4, Self::Mkv, Self::Webm, Self::Gif];

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Gif => "gif",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4",
            Self::Mkv => "MKV",
            Self::Webm => "WebM",
            Self::Gif => "GIF",
        }
    }

    /// The source's own container, so a plain Save keeps the file as it was
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("mkv") => Self::Mkv,
            Some(e) if e.eq_ignore_ascii_case("webm") => Self::Webm,
            Some(e) if e.eq_ignore_ascii_case("gif") => Self::Gif,
            _ => Self::Mp4,
        }
    }
}

/// VP9 constant-quality levels: low, medium, high. Lower is better and bigger.
pub const WEBM_CRF: &[u32] = &[40, 31, 20];
pub const GIF_FPS: &[u32] = &[30, 15, 10, 5];
pub const GIF_SCALE: &[u32] = &[100, 75, 50];

/// Cancellation handle for a running export. Cancelling sets the flag and kills ffmpeg.
#[derive(Clone, Default)]
pub struct Cancel(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    cancelled: bool,
    pid: Option<u32>,
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Signals cancellation and kills the running ffmpeg
    pub fn request(&self) {
        let mut state = self.0.lock().unwrap();
        state.cancelled = true;
        if let Some(pid) = state.pid.take() {
            kill(pid);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.0.lock().unwrap().cancelled
    }

    /// Spawns `cmd`, registering it so `request` can kill it.
    fn spawn(&self, cmd: &mut Command) -> std::io::Result<Child> {
        if self.is_cancelled() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "export cancelled",
            ));
        }
        let child = cmd.spawn()?;
        let mut state = self.0.lock().unwrap();
        if state.cancelled {
            kill(child.id());
        } else {
            state.pid = Some(child.id());
        }
        drop(state);
        Ok(child)
    }

    fn forget(&self) {
        self.0.lock().unwrap().pid = None;
    }

    /// Spawns `cmd`, waits for it, and collects its output.
    fn run(&self, cmd: &mut Command) -> std::io::Result<Output> {
        let child = self.spawn(cmd.stdout(Stdio::piped()).stderr(Stdio::piped()))?;
        let output = child.wait_with_output();
        self.forget();
        output
    }
}

fn kill(pid: u32) {
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// gifski makes far better GIFs than ffmpeg's palette, so it is used when installed.
pub fn gifski_available() -> bool {
    Command::new("gifski")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// One export, owned so it can move to a worker thread.
pub struct Request {
    pub source: PathBuf,
    pub start: f64,
    pub end: f64,
    pub speed: f64,
    pub out: PathBuf,
    /// ffmpeg writes `-progress` to this file, and the UI polls it
    pub progress_file: PathBuf,
    pub cancel: Cancel,
    pub format: Format,
    /// `WebM` only
    pub crf: u32,
    /// GIF only
    pub fps: u32,
    pub scale: u32,
    pub gifski: bool,
}

/// Builds the ffmpeg command line for `req`, writing to `tmp` (or stdout for gifski).
fn ffmpeg_args(req: &Request, tmp: &Path) -> Vec<OsString> {
    let speed_changed = (req.speed - 1.0).abs() >= 0.001;
    let setpts = speed_changed.then(|| format!("setpts={}*PTS", 1.0 / req.speed));

    let mut args: Vec<OsString> = vec![
        "-progress".into(),
        req.progress_file.clone().into(),
        "-y".into(),
        "-ss".into(),
        format!("{:.3}", req.start).into(),
        "-t".into(),
        format!("{:.3}", req.end - req.start).into(),
        "-i".into(),
        req.source.clone().into(),
    ];
    let mut push = |items: &[&str]| args.extend(items.iter().map(OsString::from));

    match req.format {
        Format::Mp4 | Format::Mkv => {
            // `-c copy` can only cut on keyframes, so it is only safe from the very start.
            // A container change is re-encoded too, so VP9 never ends up inside an MP4.
            let same_container = Format::from_path(&req.source) == req.format;
            if speed_changed || req.start > 0.001 || !same_container {
                if let Some(filter) = &setpts {
                    push(&["-filter:v", filter]);
                }
                push(&[
                    "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p",
                    "-an",
                ]);
            } else {
                push(&["-c", "copy"]);
            }
        }
        Format::Webm => {
            if let Some(filter) = &setpts {
                push(&["-filter:v", filter]);
            }
            // Not realtime-constrained like recording, so use the good-quality CRF mode.
            push(&[
                "-c:v",
                "libvpx-vp9",
                "-crf",
                &req.crf.to_string(),
                "-b:v",
                "0",
                "-deadline",
                "good",
                "-cpu-used",
                "2",
                "-row-mt",
                "1",
                "-pix_fmt",
                "yuv420p",
                "-an",
            ]);
        }
        Format::Gif => {
            let mut filters: Vec<String> = setpts.into_iter().collect();
            filters.push(format!("fps={}", req.fps));
            if req.scale < 100 {
                filters.push(format!(
                    "scale=iw*{0}/100:ih*{0}/100:flags=lanczos",
                    req.scale
                ));
            }
            if req.gifski {
                push(&["-vf", &filters.join(","), "-f", "yuv4mpegpipe", "pipe:1"]);
                return args;
            }
            filters.push("split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse=dither=bayer".into());
            push(&["-vf", &filters.join(",")]);
        }
    }

    args.push(tmp.into());
    args
}

/// Runs an export to completion on a blocking thread and returns the output size.
pub fn run(req: Request) -> Result<u64, String> {
    // A sibling temp file with the same extension, so a cancelled run never replaces the source.
    let tmp = req
        .out
        .with_extension(format!("part.{}", req.format.extension()));

    let mut ffmpeg = Command::new("ffmpeg");
    ffmpeg.args(ffmpeg_args(&req, &tmp));

    let result = if req.format == Format::Gif && req.gifski {
        run_gifski(&req, &mut ffmpeg, &tmp)
    } else {
        match req.cancel.run(&mut ffmpeg) {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(format!(
                "ffmpeg failed: {}",
                String::from_utf8_lossy(&o.stderr)
            )),
            Err(e) => Err(e.to_string()),
        }
    };
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    std::fs::rename(&tmp, &req.out).map_err(|e| e.to_string())?;
    std::fs::metadata(&req.out)
        .map(|m| m.len())
        .map_err(|e| e.to_string())
}

/// Pipes ffmpeg's decoded frames into gifski. Cancelling kills ffmpeg, and gifski
/// then stops at the end of its input.
fn run_gifski(req: &Request, ffmpeg: &mut Command, tmp: &Path) -> Result<(), String> {
    let mut decoder = req
        .cancel
        .spawn(ffmpeg.stdout(Stdio::piped()).stderr(Stdio::null()))
        .map_err(|e| e.to_string())?;
    let frames = decoder.stdout.take().ok_or("ffmpeg stdout not piped")?;
    let output = Command::new("gifski")
        .args(["--fps", &req.fps.to_string(), "-o"])
        .arg(tmp)
        .arg("-")
        .stdin(frames)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    let _ = decoder.wait();
    req.cancel.forget();
    if req.cancel.is_cancelled() {
        return Err("export cancelled".into());
    }
    match output {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!(
            "gifski failed: {}",
            String::from_utf8_lossy(&o.stderr)
        )),
        Err(e) => Err(e.to_string()),
    }
}

/// Reads the most recent encoded position, in seconds, from ffmpeg's `-progress` file.
pub fn progress_seconds(path: &Path) -> Option<f64> {
    parse_progress(&std::fs::read_to_string(path).ok()?)
}

fn parse_progress(content: &str) -> Option<f64> {
    content.lines().rev().find_map(|line| {
        line.strip_prefix("out_time_us=")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|us| us / 1_000_000.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(source: &str, format: Format, start: f64, speed: f64, gifski: bool) -> Request {
        Request {
            source: PathBuf::from(source),
            start,
            end: 5.0,
            speed,
            out: PathBuf::from(format!("out.{}", format.extension())),
            progress_file: PathBuf::from("p"),
            cancel: Cancel::new(),
            format,
            crf: 31,
            fps: 15,
            scale: 50,
            gifski,
        }
    }

    fn joined(req: &Request) -> String {
        ffmpeg_args(req, Path::new("tmp"))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn args_follow_format() {
        // untouched start, same container: stream copy
        assert!(joined(&request("a.mp4", Format::Mp4, 0.0, 1.0, false)).contains("-c copy tmp"));
        // moved start: re-encode
        assert!(joined(&request("a.mp4", Format::Mp4, 1.0, 1.0, false)).contains("libx264"));
        // container change: re-encode even from the start
        assert!(joined(&request("a.webm", Format::Mkv, 0.0, 1.0, false)).contains("libx264"));
        let webm = joined(&request("a.mp4", Format::Webm, 0.0, 2.0, false));
        assert!(webm.contains("setpts=0.5*PTS") && webm.contains("libvpx-vp9 -crf 31"));
        let gif = joined(&request("a.mp4", Format::Gif, 0.0, 1.0, false));
        assert!(gif.contains("fps=15,scale=iw*50/100:ih*50/100:flags=lanczos,split"));
        assert!(gif.ends_with(" tmp"));
        let gifski = joined(&request("a.mp4", Format::Gif, 0.0, 1.0, true));
        assert!(gifski.ends_with("-f yuv4mpegpipe pipe:1"));
    }
}
