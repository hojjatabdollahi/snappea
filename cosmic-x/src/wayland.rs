// SPDX-License-Identifier: GPL-3.0-only

use cosmic_client_toolkit::screencopy::CaptureFrame;
use cosmic_client_toolkit::screencopy::CaptureOptions;
use cosmic_client_toolkit::screencopy::CaptureSession;
use cosmic_client_toolkit::screencopy::Capturer;
use cosmic_client_toolkit::screencopy::FailureReason;
use cosmic_client_toolkit::screencopy::Formats;
use cosmic_client_toolkit::screencopy::Frame;
use cosmic_client_toolkit::screencopy::ScreencopyFrameData;
use cosmic_client_toolkit::screencopy::ScreencopyFrameDataExt;
use cosmic_client_toolkit::screencopy::ScreencopyHandler;
use cosmic_client_toolkit::screencopy::ScreencopySessionData;
use cosmic_client_toolkit::screencopy::ScreencopySessionDataExt;
use cosmic_client_toolkit::screencopy::ScreencopyState;
pub use cosmic_client_toolkit::screencopy::{CaptureSource, Rect};
use cosmic_client_toolkit::sctk;
use cosmic_client_toolkit::sctk::output::OutputHandler;
use cosmic_client_toolkit::sctk::output::OutputInfo;
use cosmic_client_toolkit::sctk::output::OutputState;
use cosmic_client_toolkit::sctk::registry::ProvidesRegistryState;
use cosmic_client_toolkit::sctk::registry::RegistryState;
use cosmic_client_toolkit::sctk::shm::Shm;
use cosmic_client_toolkit::sctk::shm::ShmHandler;
use cosmic_client_toolkit::toplevel_info::ToplevelInfo;
use cosmic_client_toolkit::toplevel_info::ToplevelInfoHandler;
use cosmic_client_toolkit::toplevel_info::ToplevelInfoState;
use cosmic_client_toolkit::workspace::WorkspaceHandler;
use cosmic_client_toolkit::workspace::WorkspaceState;
use futures::channel::oneshot;
use std::collections::HashMap;
use std::os::fd::AsFd;
use std::os::fd::BorrowedFd;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::Weak;
use std::thread;
use wayland_client::Connection;
use wayland_client::Dispatch;
use wayland_client::QueueHandle;
use wayland_client::WEnum;
use wayland_client::globals::GlobalList;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_buffer;
use wayland_client::protocol::wl_output;
use wayland_client::protocol::wl_shm;
use wayland_client::protocol::wl_shm_pool;
pub use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1;
use wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1;

/// State for linux-dmabuf protocol
#[derive(Default)]
struct DmabufState {
    /// The linux-dmabuf global (if available)
    dmabuf: Option<ZwpLinuxDmabufV1>,
}

struct WaylandHelperInner {
    conn: wayland_client::Connection,
    outputs: Mutex<Vec<wl_output::WlOutput>>,
    output_infos: Mutex<HashMap<wl_output::WlOutput, OutputInfo>>,
    output_toplevels: Mutex<HashMap<wl_output::WlOutput, Vec<ExtForeignToplevelHandleV1>>>,
    toplevels: Mutex<Vec<ToplevelInfo>>,
    qh: QueueHandle<AppData>,
    capturer: Capturer,
    wl_shm: wl_shm::WlShm,
    dmabuf_state: Mutex<DmabufState>,
}

#[derive(Clone)]
pub struct WaylandHelper {
    inner: Arc<WaylandHelperInner>,
}

struct AppData {
    wayland_helper: WaylandHelper,
    registry_state: RegistryState,
    screencopy_state: ScreencopyState,
    output_state: OutputState,
    shm_state: Shm,
    toplevel_info_state: ToplevelInfoState,
    workspace_state: WorkspaceState,
}

impl AppData {
    pub fn update_output_toplevels(&self) {
        let toplevels = self.toplevel_info_state.toplevels();
        let mut guard = self
            .wayland_helper
            .inner
            .as_ref()
            .output_toplevels
            .lock()
            .unwrap();
        *guard = toplevels
            .filter_map(|info| {
                let o = self.workspace_state.workspace_groups().find_map(|wg| {
                    wg.workspaces
                        .iter()
                        .filter_map(|handle| self.workspace_state.workspace_info(handle))
                        .find_map(|w| {
                            info.workspace
                                .iter()
                                .any(|x| {
                                    x == &w.handle
                                        && w.state.contains(ext_workspace_handle_v1::State::Active)
                                })
                                .then(|| info.output.iter().cloned().collect::<Vec<_>>())
                        })
                })?;

                Some((o, info.foreign_toplevel.clone()))
            })
            .fold(
                std::collections::HashMap::new(),
                |mut map, (outputs, toplevel)| {
                    for o in outputs {
                        map.entry(o).or_default().push(toplevel.clone());
                    }
                    map
                },
            );

        *self.wayland_helper.inner.toplevels.lock().unwrap() =
            self.toplevel_info_state.toplevels().cloned().collect();
    }
}

#[derive(Default)]
struct SessionState {
    formats: Option<Formats>,
    stopped: bool,
    wakers: Vec<std::task::Waker>,
}

struct SessionInner {
    wayland_helper: WaylandHelper,
    capture_session: CaptureSession,
    condvar: Condvar,
    state: Mutex<SessionState>,
}

pub struct Session(Arc<SessionInner>);

impl Session {
    pub fn for_session(session: &CaptureSession) -> Option<Self> {
        session.data::<SessionData>()?.session.upgrade().map(Self)
    }

    fn update<F: FnOnce(&mut SessionState)>(&self, f: F) {
        let mut state = self.0.state.lock().unwrap();
        f(&mut state);
        for waker in std::mem::take(&mut state.wakers) {
            waker.wake();
        }
        self.0.condvar.notify_all();
    }

    pub async fn wait_for_formats<T, F: FnMut(&Formats) -> T>(&self, mut cb: F) -> Option<T> {
        std::future::poll_fn(|context| {
            let mut state = self.0.state.lock().unwrap();
            if state.stopped {
                std::task::Poll::Ready(None)
            } else if let Some(formats) = &state.formats {
                std::task::Poll::Ready(Some(cb(formats)))
            } else {
                state.wakers.push(context.waker().clone());
                std::task::Poll::Pending
            }
        })
        .await
    }

    pub async fn capture_wl_buffer(
        &self,
        buffer: &wl_buffer::WlBuffer,
        buffer_damage: &[Rect],
    ) -> Result<Frame, WEnum<FailureReason>> {
        let (sender, receiver) = oneshot::channel();
        self.0.capture_session.capture(
            buffer,
            buffer_damage,
            &self.0.wayland_helper.inner.qh,
            FrameData {
                frame_data: Default::default(),
                sender: Mutex::new(Some(sender)),
            },
        );
        self.0.wayland_helper.inner.conn.flush().unwrap();

        receiver.await.unwrap()
    }
}

impl WaylandHelper {
    pub fn new(conn: wayland_client::Connection) -> Self {
        let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
        let qh = event_queue.handle();
        let registry_state = RegistryState::new(&globals);
        let screencopy_state = ScreencopyState::new(&globals, &qh);
        let shm_state = Shm::bind(&globals, &qh).unwrap();
        // Try to bind linux-dmabuf protocol (optional, not all compositors support it)
        let dmabuf_state = Self::bind_dmabuf(&globals, &qh);

        let wayland_helper = Self {
            inner: Arc::new(WaylandHelperInner {
                conn,
                outputs: Mutex::new(Vec::new()),
                output_infos: Mutex::new(HashMap::new()),
                output_toplevels: Mutex::new(HashMap::new()),
                toplevels: Mutex::new(Vec::new()),
                qh: qh.clone(),
                capturer: screencopy_state.capturer().clone(),
                wl_shm: shm_state.wl_shm().clone(),
                dmabuf_state: Mutex::new(dmabuf_state),
            }),
        };
        let mut data = AppData {
            output_state: OutputState::new(&globals, &qh),
            shm_state,
            wayland_helper: wayland_helper.clone(),
            screencopy_state,
            workspace_state: WorkspaceState::new(&registry_state, &qh),
            toplevel_info_state: ToplevelInfoState::new(&registry_state, &qh),
            registry_state,
        };
        event_queue.flush().unwrap();
        event_queue.roundtrip(&mut data).unwrap();

        // Do another roundtrip to receive dmabuf format events
        event_queue.roundtrip(&mut data).unwrap();

        // Do additional roundtrips to receive toplevel info
        // The toplevel protocol requires multiple roundtrips to get all window info
        for _ in 0..3 {
            event_queue.roundtrip(&mut data).unwrap();
        }

        thread::spawn(move || {
            loop {
                event_queue.blocking_dispatch(&mut data).unwrap();
            }
        });

        wayland_helper
    }

    /// Try to bind the linux-dmabuf protocol
    fn bind_dmabuf(globals: &GlobalList, qh: &QueueHandle<AppData>) -> DmabufState {
        // Try to bind zwp_linux_dmabuf_v1 version 3 or higher (for modifier support)
        match globals.bind::<ZwpLinuxDmabufV1, _, _>(qh, 3..=5, ()) {
            Ok(dmabuf) => {
                log::info!("linux-dmabuf protocol available");
                DmabufState {
                    dmabuf: Some(dmabuf),
                }
            }
            Err(e) => {
                log::debug!("linux-dmabuf protocol not available: {e}");
                DmabufState::default()
            }
        }
    }

    pub fn outputs(&self) -> Vec<wl_output::WlOutput> {
        self.inner.outputs.lock().unwrap().clone()
    }

    pub fn output_info(&self, output: &wl_output::WlOutput) -> Option<OutputInfo> {
        self.inner.output_infos.lock().unwrap().get(output).cloned()
    }

    /// Get all toplevels (windows) including minimized ones
    /// This returns ALL known toplevels, not just those on active workspaces
    pub fn all_toplevels(&self) -> Vec<ExtForeignToplevelHandleV1> {
        self.inner
            .toplevels
            .lock()
            .unwrap()
            .iter()
            .map(|info| info.foreign_toplevel.clone())
            .collect()
    }

    /// Wait for toplevels to be populated, with a timeout
    /// Returns true if toplevels were found, false if timeout expired
    pub fn wait_for_toplevels(&self, timeout: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        let poll_interval = std::time::Duration::from_millis(50);

        while start.elapsed() < timeout {
            let count = self.inner.toplevels.lock().unwrap().len();
            if count > 0 {
                log::debug!("Found {} toplevels after {:?}", count, start.elapsed());
                return true;
            }
            std::thread::sleep(poll_interval);
        }

        log::warn!("Timeout waiting for toplevels after {timeout:?}");
        false
    }

    fn set_output_info(&self, output: &wl_output::WlOutput, output_info_opt: Option<OutputInfo>) {
        let mut output_infos = self.inner.output_infos.lock().unwrap();
        match output_info_opt {
            Some(output_info) => {
                output_infos.insert(output.clone(), output_info);
            }
            None => {
                output_infos.remove(output);
            }
        }
    }

    pub fn capture_source_session(&self, source: CaptureSource, overlay_cursor: bool) -> Session {
        Session(Arc::new_cyclic(|weak_session| {
            let options = if overlay_cursor {
                let opts = CaptureOptions::PaintCursors;
                log::info!(
                    "Creating screencopy session WITH cursor painting enabled (options bits: {:?})",
                    opts.bits()
                );
                opts
            } else {
                log::info!("Creating screencopy session WITHOUT cursor painting (options bits: 0)");
                CaptureOptions::empty()
            };
            let capture_session = self
                .inner
                .capturer
                .create_session(
                    &source,
                    options,
                    &self.inner.qh,
                    SessionData {
                        session: weak_session.clone(),
                        session_data: Default::default(),
                    },
                )
                .unwrap();

            self.inner.conn.flush().unwrap();

            SessionInner {
                wayland_helper: self.clone(),
                capture_session,
                condvar: Condvar::new(),
                state: Default::default(),
            }
        }))
    }

    pub async fn capture_source_shm(
        &self,
        source: CaptureSource,
        overlay_cursor: bool,
    ) -> Option<ShmImage<OwnedFd>> {
        let session = self.capture_source_session(source, overlay_cursor);

        let (width, height) = session
            .wait_for_formats(|formats| formats.buffer_size)
            .await?;

        let fd = create_memfd(width, height);
        let buffer =
            self.create_shm_buffer(&fd, width, height, width * 4, wl_shm::Format::Abgr8888);

        let damage = &[Rect {
            x: 0,
            y: 0,
            width: width as i32,
            height: height as i32,
        }];
        let res = session.capture_wl_buffer(&buffer, damage).await;
        buffer.destroy();

        if let Ok(frame) = res {
            let transform = match frame.transform {
                WEnum::Value(value) => value,
                WEnum::Unknown(value) => panic!("invalid capture transform: {value}"),
            };
            Some(ShmImage {
                fd,
                width,
                height,
                transform,
            })
        } else {
            None
        }
    }

    pub fn create_shm_buffer<Fd: AsFd>(
        &self,
        fd: &Fd,
        width: u32,
        height: u32,
        stride: u32,
        format: wl_shm::Format,
    ) -> wl_buffer::WlBuffer {
        let pool = self.inner.wl_shm.create_pool(
            fd.as_fd(),
            stride as i32 * height as i32,
            &self.inner.qh,
            (),
        );
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            format,
            &self.inner.qh,
            (),
        );

        pool.destroy();

        buffer
    }

    /// Check if linux-dmabuf protocol is available
    pub fn has_dmabuf_support(&self) -> bool {
        self.inner.dmabuf_state.lock().unwrap().dmabuf.is_some()
    }

    /// Create a `wl_buffer` backed by a DMA-buf, or `None` without dmabuf support.
    pub fn create_dmabuf_buffer(
        &self,
        fd: BorrowedFd<'_>,
        width: u32,
        height: u32,
        stride: u32,
        offset: u32,
        fourcc: u32,
        modifier: u64,
    ) -> Option<wl_buffer::WlBuffer> {
        let dmabuf_state = self.inner.dmabuf_state.lock().unwrap();
        let dmabuf = dmabuf_state.dmabuf.as_ref()?;

        // Create buffer params object
        let params = dmabuf.create_params(&self.inner.qh, ());

        // Add the single plane (plane 0)
        // For multi-planar formats (like NV12), you'd add multiple planes
        let modifier_hi = (modifier >> 32) as u32;
        let modifier_lo = (modifier & 0xFFFF_FFFF) as u32;
        params.add(
            fd,
            0, // plane index
            offset,
            stride,
            modifier_hi,
            modifier_lo,
        );

        // Create the buffer immediately (create_immed)
        // This is synchronous: if it fails, the compositor will send an error
        let buffer = params.create_immed(
            width as i32,
            height as i32,
            fourcc,
            zwp_linux_buffer_params_v1::Flags::empty(),
            &self.inner.qh,
            (),
        );

        // Flush to ensure the buffer is created
        self.inner.conn.flush().ok();

        Some(buffer)
    }
}

pub struct ShmImage<T: AsFd> {
    fd: T,
    pub width: u32,
    pub height: u32,
    pub transform: wl_output::Transform,
}

impl<T: AsFd> ShmImage<T> {
    pub fn image(&self) -> anyhow::Result<image::RgbaImage> {
        let mmap = unsafe { memmap2::Mmap::map(&self.fd.as_fd())? };
        image::RgbaImage::from_raw(self.width, self.height, mmap.to_vec())
            .ok_or_else(|| anyhow::anyhow!("ShmImage had incorrect size"))
    }

    pub fn image_transformed(&self) -> anyhow::Result<image::RgbaImage> {
        let mut image = image::DynamicImage::from(self.image()?);
        image.apply_orientation(match self.transform {
            wl_output::Transform::Normal => image::metadata::Orientation::NoTransforms,
            wl_output::Transform::_90 => image::metadata::Orientation::Rotate90,
            wl_output::Transform::_180 => image::metadata::Orientation::Rotate180,
            wl_output::Transform::_270 => image::metadata::Orientation::Rotate270,
            wl_output::Transform::Flipped => image::metadata::Orientation::FlipHorizontal,
            wl_output::Transform::Flipped90 => image::metadata::Orientation::Rotate90FlipH,
            wl_output::Transform::Flipped180 => image::metadata::Orientation::FlipVertical,
            wl_output::Transform::Flipped270 => image::metadata::Orientation::Rotate270FlipH,
            _ => unreachable!(),
        });
        match image {
            image::DynamicImage::ImageRgba8(image) => Ok(image),
            _ => unreachable!(),
        }
    }
}

impl ProvidesRegistryState for AppData {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    sctk::registry_handlers!(OutputState);
}

impl ShmHandler for AppData {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

impl OutputHandler for AppData {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        let output_info_opt = self.output_state.info(&output);
        self.wayland_helper
            .set_output_info(&output, output_info_opt);

        self.wayland_helper
            .inner
            .outputs
            .lock()
            .unwrap()
            .push(output);
        self.update_output_toplevels();
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        let output_info_opt = self.output_state.info(&output);
        self.wayland_helper
            .set_output_info(&output, output_info_opt);
        self.update_output_toplevels();
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.wayland_helper.set_output_info(&output, None);

        let mut outputs = self.wayland_helper.inner.outputs.lock().unwrap();
        let idx = outputs.iter().position(|x| x == &output).unwrap();
        outputs.remove(idx);
        self.update_output_toplevels();
    }
}

impl ScreencopyHandler for AppData {
    fn screencopy_state(&mut self) -> &mut ScreencopyState {
        &mut self.screencopy_state
    }

    fn init_done(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        session: &CaptureSession,
        formats: &Formats,
    ) {
        if let Some(session) = Session::for_session(session) {
            session.update(|data| {
                data.formats = Some(formats.clone());
            });
        }
    }

    fn stopped(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, session: &CaptureSession) {
        if let Some(session) = Session::for_session(session) {
            session.update(|data| {
                data.stopped = true;
            });
        }
    }

    fn ready(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        screencopy_frame: &CaptureFrame,
        frame: Frame,
    ) {
        if let Some(sender) = screencopy_frame
            .data::<FrameData>()
            .and_then(|data| data.sender.lock().unwrap().take())
        {
            let _ = sender.send(Ok(frame));
        }
    }

    fn failed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        screencopy_frame: &CaptureFrame,
        reason: WEnum<FailureReason>,
    ) {
        if let Some(sender) = screencopy_frame
            .data::<FrameData>()
            .and_then(|data| data.sender.lock().unwrap().take())
        {
            let _ = sender.send(Err(reason));
        }
    }
}

impl ToplevelInfoHandler for AppData {
    fn toplevel_info_state(&mut self) -> &mut ToplevelInfoState {
        &mut self.toplevel_info_state
    }

    fn new_toplevel(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _toplevel: &ExtForeignToplevelHandleV1,
    ) {
        self.update_output_toplevels();
    }

    fn update_toplevel(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _toplevel: &ExtForeignToplevelHandleV1,
    ) {
        self.update_output_toplevels();
    }

    fn toplevel_closed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _toplevel: &ExtForeignToplevelHandleV1,
    ) {
        self.update_output_toplevels();
    }
}

impl WorkspaceHandler for AppData {
    fn workspace_state(&mut self) -> &mut WorkspaceState {
        &mut self.workspace_state
    }

    fn done(&mut self) {
        self.update_output_toplevels();
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for AppData {
    fn event(
        _app_data: &mut Self,
        _buffer: &wl_shm_pool::WlShmPool,
        _event: wl_shm_pool::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for AppData {
    fn event(
        _app_data: &mut Self,
        _buffer: &wl_buffer::WlBuffer,
        _event: wl_buffer::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

// Linux DMA-buf protocol dispatch implementations

impl Dispatch<ZwpLinuxDmabufV1, ()> for AppData {
    fn event(
        _app_data: &mut Self,
        _proxy: &ZwpLinuxDmabufV1,
        _event: zwp_linux_dmabuf_v1::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpLinuxBufferParamsV1, ()> for AppData {
    fn event(
        _app_data: &mut Self,
        params: &ZwpLinuxBufferParamsV1,
        event: zwp_linux_buffer_params_v1::Event,
        (): &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwp_linux_buffer_params_v1::Event::Created { buffer: _ } => {
                // Buffer was created successfully (only for async create, not create_immed)
                log::debug!("DMA-buf buffer created successfully");
            }
            zwp_linux_buffer_params_v1::Event::Failed => {
                // Buffer creation failed
                log::warn!("DMA-buf buffer creation failed");
            }
            _ => {}
        }
        // Destroy params after use
        params.destroy();
    }
}

struct SessionData {
    session: Weak<SessionInner>,
    session_data: ScreencopySessionData,
}

impl ScreencopySessionDataExt for SessionData {
    fn screencopy_session_data(&self) -> &ScreencopySessionData {
        &self.session_data
    }
}

struct FrameData {
    frame_data: ScreencopyFrameData,
    #[allow(clippy::type_complexity)]
    sender: Mutex<Option<oneshot::Sender<Result<Frame, WEnum<FailureReason>>>>>,
}

impl ScreencopyFrameDataExt for FrameData {
    fn screencopy_frame_data(&self) -> &ScreencopyFrameData {
        &self.frame_data
    }
}

sctk::delegate_shm!(AppData);
sctk::delegate_registry!(AppData);
sctk::delegate_output!(AppData);
cosmic_client_toolkit::delegate_screencopy!(AppData);
cosmic_client_toolkit::delegate_toplevel_info!(AppData);
cosmic_client_toolkit::delegate_workspace!(AppData);

pub fn create_memfd(width: u32, height: u32) -> OwnedFd {
    // TODO: BSD support using shm_open
    let name = c"cosmic-x-screencopy";
    let fd = rustix::fs::memfd_create(name, rustix::fs::MemfdFlags::CLOEXEC).unwrap();
    rustix::fs::ftruncate(&fd, (width * height * 4).into()).unwrap();
    fd
}
