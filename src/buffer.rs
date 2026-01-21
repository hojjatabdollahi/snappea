use std::os::fd::OwnedFd;

pub fn create_memfd(width: u32, height: u32) -> OwnedFd {
    // TODO: BSD support using shm_open
    let name = c"snappea-screencopy";
    let fd = rustix::fs::memfd_create(name, rustix::fs::MemfdFlags::CLOEXEC).unwrap();
    rustix::fs::ftruncate(&fd, (width * height * 4) as _).unwrap();
    fd
}
