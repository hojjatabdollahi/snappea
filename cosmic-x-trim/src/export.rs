// SPDX-License-Identifier: GPL-3.0-only

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};

/// Output container
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

    /// Video codecs the container holds. `WebM` only takes VP8/VP9/AV1.
    pub const fn codecs(self) -> &'static [Codec] {
        match self {
            Self::Mp4 => &[Codec::H264, Codec::Av1],
            Self::Mkv => &[Codec::H264, Codec::Vp9, Codec::Av1],
            Self::Webm => &[Codec::Vp9, Codec::Av1],
            Self::Gif => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    Vp9,
    Av1,
}

impl Codec {
    pub const fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::Vp9 => "VP9",
            Self::Av1 => "AV1",
        }
    }

    fn from_ffprobe(name: &str) -> Option<Self> {
        match name.trim() {
            "h264" => Some(Self::H264),
            "vp9" => Some(Self::Vp9),
            "av1" => Some(Self::Av1),
            _ => None,
        }
    }
}

/// An ffmpeg video encoder the export can use
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderKind {
    X264,
    H264Nvenc,
    H264Vaapi,
    Vpx,
    Vp9Vaapi,
    SvtAv1,
    Av1Nvenc,
    Av1Vaapi,
}

impl EncoderKind {
    /// Software first within each codec, so it is the default choice
    pub const ALL: [Self; 8] = [
        Self::X264,
        Self::H264Nvenc,
        Self::H264Vaapi,
        Self::Vpx,
        Self::Vp9Vaapi,
        Self::SvtAv1,
        Self::Av1Nvenc,
        Self::Av1Vaapi,
    ];

    const fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::X264 => "libx264",
            Self::H264Nvenc => "h264_nvenc",
            Self::H264Vaapi => "h264_vaapi",
            Self::Vpx => "libvpx-vp9",
            Self::Vp9Vaapi => "vp9_vaapi",
            Self::SvtAv1 => "libsvtav1",
            Self::Av1Nvenc => "av1_nvenc",
            Self::Av1Vaapi => "av1_vaapi",
        }
    }

    pub const fn codec(self) -> Codec {
        match self {
            Self::X264 | Self::H264Nvenc | Self::H264Vaapi => Codec::H264,
            Self::Vpx | Self::Vp9Vaapi => Codec::Vp9,
            Self::SvtAv1 | Self::Av1Nvenc | Self::Av1Vaapi => Codec::Av1,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::X264 => "x264",
            Self::Vpx => "libvpx",
            Self::SvtAv1 => "SVT-AV1",
            Self::H264Nvenc | Self::Av1Nvenc => "NVENC",
            Self::H264Vaapi | Self::Vp9Vaapi | Self::Av1Vaapi => "VA-API",
        }
    }

    pub const fn is_hardware(self) -> bool {
        !matches!(self, Self::X264 | Self::Vpx | Self::SvtAv1)
    }

    const fn is_vaapi(self) -> bool {
        matches!(self, Self::H264Vaapi | Self::Vp9Vaapi | Self::Av1Vaapi)
    }

    /// Rate-control value for each `QUALITY_LEVELS` entry, lower is better. Levels
    /// were matched by SSIM on a 4K screen recording. `Av1Vaapi` reuses the VP9 scale, untested.
    const fn quality_scale(self) -> [u32; QUALITY_LEVELS] {
        match self {
            Self::X264 => [23, 18, 15],
            Self::H264Nvenc => [30, 23, 19],
            Self::H264Vaapi => [24, 20, 16],
            Self::Vpx => [40, 31, 20],
            Self::Vp9Vaapi | Self::Av1Vaapi => [100, 60, 40],
            Self::SvtAv1 => [45, 38, 30],
            Self::Av1Nvenc => [38, 30, 24],
        }
    }

    fn args(self, quality: usize) -> Vec<String> {
        let q = self.quality_scale()[quality.min(QUALITY_LEVELS - 1)].to_string();
        let args: &[&str] = match self {
            Self::X264 => &["-preset", "veryfast", "-crf", &q],
            // Not realtime-constrained like recording, so use the good-quality CRF mode.
            Self::Vpx => &[
                "-crf",
                &q,
                "-b:v",
                "0",
                "-deadline",
                "good",
                "-cpu-used",
                "2",
                "-row-mt",
                "1",
            ],
            Self::SvtAv1 => &["-preset", "8", "-crf", &q],
            Self::H264Nvenc => &["-preset", "p4", "-rc", "vbr", "-cq", &q, "-b:v", "0"],
            Self::Av1Nvenc => &["-preset", "p5", "-rc", "vbr", "-cq", &q, "-b:v", "0"],
            Self::H264Vaapi | Self::Vp9Vaapi | Self::Av1Vaapi => {
                &["-rc_mode", "CQP", "-global_quality", &q]
            }
        };
        let mut args: Vec<String> = args.iter().map(ToString::to_string).collect();
        // VA-API takes nv12 surfaces from `hwupload` instead.
        if !self.is_vaapi() {
            args.extend(["-pix_fmt".into(), "yuv420p".into()]);
        }
        args
    }
}

/// An encoder that works on this machine
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoder {
    pub kind: EncoderKind,
    /// The VA-API render node it worked on
    device: Option<PathBuf>,
}

impl Encoder {
    /// Assumed before `probe_encoders` finishes, as the export always needed them
    pub fn fallback() -> Vec<Self> {
        [EncoderKind::X264, EncoderKind::Vpx]
            .into_iter()
            .map(|kind| Self { kind, device: None })
            .collect()
    }

    /// Encodes one tiny frame, which fails fast when the driver or GPU lacks it
    fn probe(kind: EncoderKind, device: Option<&Path>) -> bool {
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error"]);
        if let Some(device) = device {
            cmd.arg("-vaapi_device").arg(device);
        }
        cmd.args(["-f", "lavfi", "-i", "color=black:s=256x256:d=0.1"]);
        if device.is_some() {
            cmd.args(["-vf", "format=nv12,hwupload"]);
        }
        cmd.args([
            "-frames:v",
            "1",
            "-c:v",
            kind.ffmpeg_name(),
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    }
}

/// Every encoder that works here, in `EncoderKind::ALL` order. Probes run in parallel.
pub fn probe_encoders() -> Vec<Encoder> {
    let mut render_nodes: Vec<PathBuf> = std::fs::read_dir("/dev/dri")
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("renderD"))
        })
        .collect();
    render_nodes.sort();

    std::thread::scope(|scope| {
        // Collected so every probe is spawned before the first join.
        #[allow(clippy::needless_collect)]
        let probes: Vec<_> = EncoderKind::ALL
            .into_iter()
            .map(|kind| {
                let render_nodes = &render_nodes;
                scope.spawn(move || {
                    if kind.is_vaapi() {
                        render_nodes
                            .iter()
                            .find(|node| Encoder::probe(kind, Some(node)))
                            .map(|node| Encoder {
                                kind,
                                device: Some(node.clone()),
                            })
                    } else {
                        Encoder::probe(kind, None).then_some(Encoder { kind, device: None })
                    }
                })
            })
            .collect();
        probes
            .into_iter()
            .filter_map(|probe| probe.join().ok().flatten())
            .collect()
    })
}

/// The video codec of `path`'s first video stream
pub fn source_codec(path: &Path) -> Option<Codec> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Codec::from_ffprobe(&String::from_utf8_lossy(&output.stdout))
}

/// Low, medium, high
pub const QUALITY_LEVELS: usize = 3;
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
    /// Lets an untouched same-codec export skip re-encoding
    pub source_codec: Option<Codec>,
    pub start: f64,
    pub end: f64,
    pub speed: f64,
    pub out: PathBuf,
    /// ffmpeg writes `-progress` to this file, and the UI polls it
    pub progress_file: PathBuf,
    pub cancel: Cancel,
    pub format: Format,
    /// Every format but GIF
    pub encoder: Option<Encoder>,
    /// index into the encoder's `QUALITY_LEVELS`
    pub quality: usize,
    /// GIF only
    pub fps: u32,
    pub scale: u32,
    pub gifski: bool,
}

/// Builds the ffmpeg command line for `req`, writing to `tmp` (or stdout for gifski).
fn ffmpeg_args(req: &Request, tmp: &Path) -> Vec<OsString> {
    let speed_changed = (req.speed - 1.0).abs() >= 0.001;
    let setpts = speed_changed.then(|| format!("setpts={}*PTS", 1.0 / req.speed));
    let device = req.encoder.as_ref().and_then(|e| e.device.as_ref());

    let mut args: Vec<OsString> = vec![
        "-progress".into(),
        req.progress_file.clone().into(),
        "-y".into(),
    ];
    if let Some(device) = device {
        args.extend(["-vaapi_device".into(), device.clone().into()]);
    }
    args.extend([
        "-ss".into(),
        format!("{:.3}", req.start).into(),
        "-t".into(),
        format!("{:.3}", req.end - req.start).into(),
        "-i".into(),
        req.source.clone().into(),
    ]);
    let mut push = |items: &[&str]| args.extend(items.iter().map(OsString::from));

    if req.format == Format::Gif {
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
    } else if let Some(encoder) = &req.encoder {
        // `-c copy` can only cut on keyframes, so it is only safe from the very start,
        // and only when the stream already is what was asked for.
        let copy = !speed_changed
            && req.start <= 0.001
            && Format::from_path(&req.source) == req.format
            && req.source_codec == Some(encoder.kind.codec());
        if copy {
            push(&["-c", "copy"]);
        } else {
            let mut filters: Vec<String> = setpts.into_iter().collect();
            if device.is_some() {
                filters.push("format=nv12,hwupload".into());
            }
            if !filters.is_empty() {
                push(&["-filter:v", &filters.join(",")]);
            }
            push(&["-c:v", encoder.kind.ffmpeg_name()]);
            args.extend(
                encoder
                    .kind
                    .args(req.quality)
                    .into_iter()
                    .map(OsString::from),
            );
            args.push("-an".into());
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

    fn encoder(kind: EncoderKind) -> Encoder {
        let device = kind
            .is_vaapi()
            .then(|| PathBuf::from("/dev/dri/renderD128"));
        Encoder { kind, device }
    }

    fn request(source: &str, format: Format, start: f64, speed: f64, gifski: bool) -> Request {
        let kind = match format {
            Format::Webm => EncoderKind::Vpx,
            _ => EncoderKind::X264,
        };
        Request {
            source: PathBuf::from(source),
            source_codec: Some(Codec::H264),
            start,
            end: 5.0,
            speed,
            out: PathBuf::from(format!("out.{}", format.extension())),
            progress_file: PathBuf::from("p"),
            cancel: Cancel::new(),
            format,
            encoder: (format != Format::Gif).then(|| encoder(kind)),
            quality: 1,
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
        assert!(!gifski.contains("-c:v"));
    }

    #[test]
    fn copy_needs_matching_codec() {
        let mut req = request("a.mkv", Format::Mkv, 0.0, 1.0, false);
        assert!(joined(&req).contains("-c copy tmp"));
        req.source_codec = Some(Codec::Vp9);
        assert!(joined(&req).contains("libx264"));
        req.source_codec = None;
        assert!(joined(&req).contains("libx264"));
    }

    #[test]
    fn vaapi_uploads_frames() {
        let mut req = request("a.mp4", Format::Webm, 1.0, 2.0, false);
        req.encoder = Some(encoder(EncoderKind::Vp9Vaapi));
        let args = joined(&req);
        assert!(args.starts_with("-progress p -y -vaapi_device /dev/dri/renderD128 -ss"));
        assert!(args.contains("-filter:v setpts=0.5*PTS,format=nv12,hwupload -c:v vp9_vaapi"));
        assert!(args.contains("-global_quality 60"));
        assert!(!args.contains("-pix_fmt"));
    }

    #[test]
    fn nvenc_quality_levels() {
        let mut req = request("a.mp4", Format::Webm, 1.0, 1.0, false);
        req.encoder = Some(encoder(EncoderKind::Av1Nvenc));
        req.quality = 2;
        let args = joined(&req);
        assert!(args.contains("-c:v av1_nvenc -preset p5 -rc vbr -cq 24"));
        assert!(args.contains("-pix_fmt yuv420p") && !args.contains("hwupload"));
    }

    #[test]
    fn formats_hold_their_codecs() {
        assert!(!Format::Webm.codecs().contains(&Codec::H264));
        assert!(!Format::Mp4.codecs().contains(&Codec::Vp9));
        for kind in EncoderKind::ALL {
            assert!(Format::Mkv.codecs().contains(&kind.codec()));
        }
    }
}
