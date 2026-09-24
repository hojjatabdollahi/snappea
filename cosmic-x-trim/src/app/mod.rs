// SPDX-License-Identifier: GPL-3.0-only

mod view;

use crate::export::{self, Cancel, Codec, Encoder, Format};
use cosmic::app::Task;
use cosmic::{Application, executor};
use iced_video_player::Video;
use std::path::PathBuf;
use std::time::Duration;

pub const SPEED_OPTIONS: &[f64] = &[0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0];

/// limit preview seeks so scrubbing cannot build a `GStreamer` backlog
const SEEK_PREVIEW_INTERVAL: Duration = Duration::from_millis(50);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);

pub struct Flags {
    pub path: PathBuf,
    pub can_discard: bool,
}

/// When saving/converting using ffmpeg
struct Export {
    fraction: f32,
    output_secs: f64,
    progress_file: PathBuf,
    ticks: u32,
    out: PathBuf,
    cancel: Cancel,
}

pub struct Editor {
    core: cosmic::Core,
    path: PathBuf,
    video: Video,
    can_discard: bool,
    duration: f64,
    position: f64,
    trim_start: f64,
    trim_end: f64,
    /// we have one span in ctulet, so one speed
    speed: f64,
    playing: bool,
    dragging: bool,

    /// Latest scrub position to be sent to `GStreamer`
    pending_seek: Option<f64>,
    seek_scheduled: bool,
    seek_generation: u64,

    speed_menu_open: bool,
    /// for the timeline readability
    frame_colors: Vec<[u8; 3]>,

    export: Option<Export>,
    export_generation: u64,

    source_codec: Option<Codec>,
    /// Encoders that work here, `Encoder::fallback` until the probe finishes
    encoders: Vec<Encoder>,

    /// Chosen in the export drawer. Save and Save As keep the source's container.
    format: Format,
    /// `None` for GIF
    encoder: Option<Encoder>,
    /// index into `export::QUALITY_LEVELS`
    quality: usize,
    /// indices into `export::GIF_FPS` and `GIF_SCALE`
    gif_fps: usize,
    gif_scale: usize,
    gifski: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    // Trimming
    Seek(f64),
    FlushSeek(u64),
    SeekReleased,
    TrimChanged(f64, f64),
    TrimReleased,
    ColorsExtracted(Vec<[u8; 3]>),
    // Speed
    ToggleSpeedMenu,
    SetSpeed(usize),
    // Playback
    TogglePlay,
    NewFrame,
    DurationChanged(Duration),
    EndOfStream,
    // Export drawer
    ToggleExportDrawer,
    SetFormat(usize),
    SetEncoder(usize),
    SetQuality(usize),
    EncodersProbed(Vec<Encoder>),
    SetGifFps(usize),
    SetGifScale(usize),
    // Export
    ExportTick,
    ExportFinished(u64, Result<u64, String>),
    CancelExport,
    // Actions
    Save,
    SaveAs,
    Export,
    SaveAsChosen(Format, Option<Encoder>, Option<PathBuf>),
    Discard,
}

impl Editor {
    const fn cancel_pending_seek(&mut self) {
        self.pending_seek = None;
        self.seek_scheduled = false;
        self.seek_generation = self.seek_generation.wrapping_add(1);
    }

    /// Smallest meaningful trim span, one frame of the loaded video
    fn frame_duration(&self) -> f64 {
        1.0 / self.video.framerate().max(1.0)
    }

    fn source_format(&self) -> Format {
        Format::from_path(&self.path)
    }

    /// The working encoders `format` can hold
    fn encoders_for(&self, format: Format) -> impl Iterator<Item = &Encoder> {
        self.encoders
            .iter()
            .filter(move |e| format.codecs().contains(&e.kind.codec()))
    }

    /// Software, and the source's codec when `format` holds it, so an untouched
    /// export can stream-copy
    fn default_encoder(&self, format: Format) -> Option<Encoder> {
        self.encoders_for(format)
            .find(|e| Some(e.kind.codec()) == self.source_codec)
            .or_else(|| self.encoders_for(format).next())
            .cloned()
    }

    /// Asks where to write a `format` file, defaulting next to the source
    fn save_dialog(&self, format: Format, encoder: Option<Encoder>) -> Task<Message> {
        let stem = self
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let default_name = format!("{stem}-trimmed.{}", format.extension());
        let start_dir = self.path.parent().map(std::path::Path::to_path_buf);
        Task::perform(
            async move {
                let mut dialog = rfd::AsyncFileDialog::new().set_file_name(&default_name);
                if let Some(dir) = start_dir {
                    dialog = dialog.set_directory(dir);
                }
                dialog.save_file().await.map(|f| f.path().to_path_buf())
            },
            move |path| cosmic::Action::App(Message::SaveAsChosen(format, encoder, path)),
        )
    }

    /// Starts an export on a worker thread and opens the progress dialog
    fn begin_export(
        &mut self,
        out: PathBuf,
        format: Format,
        encoder: Option<Encoder>,
    ) -> Task<Message> {
        if self.export.is_some() {
            return Task::none();
        }
        let cancel = Cancel::new();
        let progress_file = std::env::temp_dir().join(format!(
            "cosmic-x-trim-export-{}.progress",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&progress_file);

        self.export_generation = self.export_generation.wrapping_add(1);
        let generation = self.export_generation;
        self.export = Some(Export {
            fraction: 0.0,
            output_secs: ((self.trim_end - self.trim_start) / self.speed).max(0.0),
            progress_file: progress_file.clone(),
            ticks: 0,
            out: out.clone(),
            cancel: cancel.clone(),
        });

        let req = export::Request {
            source: self.path.clone(),
            source_codec: self.source_codec,
            start: self.trim_start,
            end: self.trim_end,
            speed: self.speed,
            out,
            progress_file,
            cancel,
            format,
            encoder,
            quality: self.quality,
            fps: export::GIF_FPS[self.gif_fps],
            scale: export::GIF_SCALE[self.gif_scale],
            gifski: self.gifski,
        };
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || export::run(req))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()))
            },
            move |res| cosmic::Action::App(Message::ExportFinished(generation, res)),
        )
    }
}

impl Application for Editor {
    type Executor = executor::Default;
    type Flags = Flags;
    type Message = Message;

    const APP_ID: &'static str = "com.system76.CosmicX.Trim";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(mut core: cosmic::Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let uri = url::Url::from_file_path(&flags.path).expect("absolute media path");
        let mut video = Video::new(&uri).unwrap_or_else(|e| {
            eprintln!("cannot open {}: {e}", flags.path.display());
            std::process::exit(1);
        });
        video.set_paused(true);
        let duration = video.duration().as_secs_f64();

        let title = format!("{} - COSMIC X Trim", flags.path.display());
        core.window.show_headerbar = true;
        core.set_header_title(title.clone());
        let title_task = core.set_title(None, title);

        let color_path = flags.path.clone();
        let colors_task = Task::perform(
            async move {
                tokio::task::spawn_blocking(move || crate::colors::extract(&color_path, duration))
                    .await
                    .unwrap_or_default()
            },
            |colors| cosmic::Action::App(Message::ColorsExtracted(colors)),
        );

        let probe_task = Task::perform(
            async {
                tokio::task::spawn_blocking(export::probe_encoders)
                    .await
                    .unwrap_or_else(|_| Encoder::fallback())
            },
            |encoders| cosmic::Action::App(Message::EncodersProbed(encoders)),
        );

        let mut editor = Self {
            core,
            source_codec: export::source_codec(&flags.path),
            encoders: Encoder::fallback(),
            format: Format::from_path(&flags.path),
            encoder: None,
            path: flags.path,
            video,
            can_discard: flags.can_discard,
            duration,
            position: 0.0,
            trim_start: 0.0,
            trim_end: duration,
            speed: 1.0,
            playing: false,
            dragging: false,
            pending_seek: None,
            seek_scheduled: false,
            seek_generation: 0,
            speed_menu_open: false,
            frame_colors: Vec::new(),
            export: None,
            export_generation: 0,
            quality: 1,
            gif_fps: 1,
            gif_scale: 0,
            gifski: export::gifski_available(),
        };
        editor.encoder = editor.default_encoder(editor.format);
        (editor, Task::batch([title_task, colors_task, probe_task]))
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::Seek(position) => {
                self.position = position.clamp(0.0, self.duration);
                self.playing = false;
                self.dragging = true;
                self.video.set_paused(true);
                self.pending_seek = Some(self.position);
                if !self.seek_scheduled {
                    self.seek_scheduled = true;
                    self.seek_generation = self.seek_generation.wrapping_add(1);
                    let generation = self.seek_generation;
                    return Task::perform(
                        async move {
                            tokio::time::sleep(SEEK_PREVIEW_INTERVAL).await;
                            generation
                        },
                        |generation| cosmic::Action::App(Message::FlushSeek(generation)),
                    );
                }
            }
            Message::FlushSeek(generation) => {
                if generation != self.seek_generation {
                    return Task::none();
                }
                self.seek_scheduled = false;
                if let Some(position) = self.pending_seek.take() {
                    // iced_video_player sets GStreamer's FLUSH flag, so this
                    // request supersedes any older preview seek.
                    let _ = self.video.seek(Duration::from_secs_f64(position), false);
                }
            }
            Message::SeekReleased => {
                self.dragging = false;
                self.cancel_pending_seek();
                let _ = self
                    .video
                    .seek(Duration::from_secs_f64(self.position), true);
            }
            Message::TrimChanged(new_start, new_end) => {
                // Follow whichever handle moved, so the frame being cut on is
                // the frame on screen.
                let follow = if (new_start - self.trim_start).abs() > f64::EPSILON {
                    new_start
                } else {
                    new_end
                };
                self.trim_start = new_start;
                self.trim_end = new_end;
                self.dragging = true;
                self.position = follow;
                self.video.set_paused(true);
            }
            Message::TrimReleased => {
                self.dragging = false;
                self.cancel_pending_seek();
                self.position = self.position.clamp(self.trim_start, self.trim_end);
                let _ = self
                    .video
                    .seek(Duration::from_secs_f64(self.position), true);
            }
            Message::ColorsExtracted(colors) => self.frame_colors = colors,

            Message::ToggleSpeedMenu => self.speed_menu_open = !self.speed_menu_open,
            Message::SetSpeed(index) => {
                self.speed = SPEED_OPTIONS[index.min(SPEED_OPTIONS.len() - 1)];
                self.speed_menu_open = false;
            }

            Message::TogglePlay => {
                self.playing = !self.playing;
                // Play always starts inside the kept span.
                if self.playing {
                    if self.position < self.trim_start || self.position >= self.trim_end {
                        self.position = self.trim_start;
                    }
                    self.video.set_paused(true);
                    let _ = self.video.set_speed(self.speed);
                    let _ = self
                        .video
                        .seek(Duration::from_secs_f64(self.position), true);
                    self.video.set_paused(false);
                } else {
                    self.video.set_paused(true);
                }
            }
            Message::NewFrame => {
                if self.dragging {
                    return Task::none();
                }
                let pos = self.video.position().as_secs_f64();
                if self.playing && !(self.trim_start..self.trim_end).contains(&pos) {
                    // Playback loops within the kept span.
                    let _ = self
                        .video
                        .seek(Duration::from_secs_f64(self.trim_start), true);
                    self.position = self.trim_start;
                } else {
                    if self.playing && (self.speed - self.video.speed()).abs() > 0.001 {
                        let _ = self.video.set_speed(self.speed);
                    }
                    self.position = pos;
                }
            }
            Message::DurationChanged(duration) => {
                let secs = duration.as_secs_f64();
                if secs > 0.0 {
                    self.duration = secs;
                    if self.trim_end <= 0.0 || self.trim_end > secs {
                        self.trim_end = secs;
                    }
                }
            }
            Message::EndOfStream => {
                if self.playing {
                    // The player has paused itself and flagged the end. A bare seek
                    // would leave it there. Restart it, then jump to the kept span.
                    let _ = self.video.restart_stream();
                    let _ = self
                        .video
                        .seek(Duration::from_secs_f64(self.trim_start), true);
                    self.position = self.trim_start;
                }
            }

            Message::ToggleExportDrawer => {
                let show = !self.core.window.show_context;
                self.core.set_show_context(show);
            }
            Message::SetFormat(index) => {
                self.format = Format::ALL[index.min(Format::ALL.len() - 1)];
                self.encoder = self.default_encoder(self.format);
            }
            Message::SetEncoder(index) => {
                let encoder = self.encoders_for(self.format).nth(index).cloned();
                if encoder.is_some() {
                    self.encoder = encoder;
                }
            }
            Message::SetQuality(index) => self.quality = index.min(export::QUALITY_LEVELS - 1),
            Message::EncodersProbed(encoders) => {
                // An empty probe means ffmpeg misbehaved, so keep the fallback.
                if !encoders.is_empty() {
                    self.encoders = encoders;
                }
                let kept = self.encoder.as_ref().and_then(|chosen| {
                    self.encoders_for(self.format)
                        .find(|e| e.kind == chosen.kind)
                        .cloned()
                });
                self.encoder = kept.or_else(|| self.default_encoder(self.format));
            }
            Message::SetGifFps(index) => self.gif_fps = index.min(export::GIF_FPS.len() - 1),
            Message::SetGifScale(index) => {
                self.gif_scale = index.min(export::GIF_SCALE.len() - 1);
            }

            Message::ExportTick => {
                if let Some(export) = &mut self.export {
                    // ffmpeg's own position when it reports one
                    // we add a fake progress so the bar never looks frozen
                    export.ticks = export.ticks.saturating_add(1);
                    let elapsed = f64::from(export.ticks) * PROGRESS_INTERVAL.as_secs_f64();
                    let creep = 0.92 * (1.0 - (-elapsed / 10.0).exp());
                    let real = if export.output_secs > 0.0 {
                        export::progress_seconds(&export.progress_file)
                            .map_or(0.0, |s| (s / export.output_secs).clamp(0.0, 0.99))
                    } else {
                        0.0
                    };
                    export.fraction = creep.max(real) as f32;
                }
            }
            Message::CancelExport => {
                if let Some(export) = self.export.take() {
                    export.cancel.request();
                    let _ = std::fs::remove_file(&export.progress_file);
                }
                // Drop the in-flight result when it arrives.
                self.export_generation = self.export_generation.wrapping_add(1);
            }
            Message::ExportFinished(generation, result) => {
                if generation != self.export_generation {
                    return Task::none();
                }
                let Some(export) = self.export.take() else {
                    return Task::none();
                };
                let _ = std::fs::remove_file(&export.progress_file);
                match result {
                    Ok(size) => log::info!("wrote {} ({size} bytes)", export.out.display()),
                    Err(e) => log::error!("export failed: {e}"),
                }
            }

            Message::Save => {
                let out = self.path.clone();
                let format = self.source_format();
                let encoder = self.default_encoder(format);
                return self.begin_export(out, format, encoder);
            }
            Message::SaveAs => {
                let format = self.source_format();
                return self.save_dialog(format, self.default_encoder(format));
            }
            Message::Export => return self.save_dialog(self.format, self.encoder.clone()),
            Message::SaveAsChosen(format, encoder, Some(path)) => {
                return self.begin_export(path, format, encoder);
            }
            Message::SaveAsChosen(_, _, None) => {}
            Message::Discard => {
                let _ = std::fs::remove_file(&self.path);
                std::process::exit(0);
            }
        }
        Task::none()
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        if self.export.is_some() {
            cosmic::iced::time::every(PROGRESS_INTERVAL).map(|_| Message::ExportTick)
        } else {
            cosmic::iced::Subscription::none()
        }
    }

    fn context_drawer(&self) -> Option<cosmic::app::context_drawer::ContextDrawer<'_, Message>> {
        self.core
            .window
            .show_context
            .then(|| view::export_drawer(self))
    }

    fn dialog(&self) -> Option<cosmic::Element<'_, Self::Message>> {
        view::dialog(self)
    }

    fn view(&self) -> cosmic::Element<'_, Self::Message> {
        view::view(self)
    }
}
