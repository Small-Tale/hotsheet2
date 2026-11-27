//! HS2-2HZN2C: a macOS FSEvents stream that leaves whole directory trees out at the source.
//!
//! An FSEvents stream is always recursive, and `notify` filters events only in its own
//! callback, after the file-event service has routed them to this process. For a checkout
//! whose ignored build output (`target/`, `node_modules/`, agent worktrees) is written
//! constantly, that routing cost CPU in both `fseventsd` and the server. The service can drop
//! those events itself through `FSEventStreamSetExclusionPaths`, which `notify` does not
//! expose, so this module creates the stream directly. Its lifecycle (a run-loop thread per
//! stream, stopped and joined on drop) is the one `notify` 8 uses.

use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Mutex;

use notify_batched::event::{
    CreateKind, DataChange, Flag, MetadataKind, ModifyKind, RemoveKind, RenameMode,
};
use notify_batched::{Error, Event, EventKind, RecursiveMode};
use objc2_core_foundation as cf;
use objc2_core_services as fs;

use crate::watch::Delivery;

use crate::watch::MAX_EXCLUDED_PATHS;

type Handler = Box<dyn FnMut(Delivery) + Send>;

struct StreamInfo {
    handler: Mutex<Handler>,
    /// Each watched path (as the service reports it) and whether its subtree is watched.
    watched: Vec<(PathBuf, bool)>,
}

/// A running stream; dropping it stops the stream and waits for its thread.
pub(crate) struct ExcludingStream {
    runloop: Option<(cf::CFRetained<cf::CFRunLoop>, std::thread::JoinHandle<()>)>,
}

impl std::fmt::Debug for ExcludingStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExcludingStream").finish_non_exhaustive()
    }
}

// The run loop is only stopped from the owning thread; Apple documents CFRunLoop as safe to
// move between threads, as `notify` relies on too.
unsafe impl Send for ExcludingStream {}

/// The spelling the file-event service reports for `path`: macOS reports `/private/var/...`
/// for `/var/...`.
fn reported(path: &Path) -> PathBuf {
    hotsheet_sync::paths::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn cf_paths<'a>(
    paths: impl IntoIterator<Item = &'a Path>,
) -> cf::CFRetained<cf::CFMutableArray<cf::CFString>> {
    let array = cf::CFMutableArray::<cf::CFString>::empty();
    for path in paths {
        array.append(&cf::CFString::from_str(&path.to_string_lossy()));
    }
    array
}

impl ExcludingStream {
    /// A stream that is not running, standing in while a replacement starts.
    pub(crate) fn stopped() -> Self {
        Self { runloop: None }
    }

    /// Start one stream over `paths` (each must exist) that never reports anything inside
    /// `excluded`. At most [`MAX_EXCLUDED_PATHS`] exclusions are accepted.
    pub(crate) fn start(
        paths: &[(&Path, RecursiveMode)],
        excluded: &[&Path],
        handler: Handler,
    ) -> Result<Self, Error> {
        if excluded.len() > MAX_EXCLUDED_PATHS {
            return Err(Error::generic("more than 8 excluded paths"));
        }
        let mut watched = Vec::with_capacity(paths.len());
        for (path, mode) in paths {
            if !path.exists() {
                return Err(Error::path_not_found().add_path(path.to_path_buf()));
            }
            watched.push((reported(path), matches!(mode, RecursiveMode::Recursive)));
        }
        if watched.is_empty() {
            return Ok(Self { runloop: None });
        }
        let watch_array = cf_paths(watched.iter().map(|(path, _)| path.as_path()));
        let excluded_paths: Vec<PathBuf> = excluded.iter().map(|path| reported(path)).collect();
        let exclude_array = cf_paths(excluded_paths.iter().map(PathBuf::as_path));
        let info = Box::into_raw(Box::new(StreamInfo {
            handler: Mutex::new(handler),
            watched,
        }));
        let context = fs::FSEventStreamContext {
            version: 0,
            info: info.cast(),
            retain: None,
            release: Some(release_info),
            copyDescription: None,
        };
        // SAFETY: the callback matches the documented signature; the context is copied by
        // the service, which owns `info` from here and frees it through `release_info`.
        let stream = unsafe {
            fs::FSEventStreamCreate(
                cf::kCFAllocatorDefault,
                Some(callback),
                &context as *const _ as *mut _,
                watch_array.as_opaque(),
                fs::kFSEventStreamEventIdSinceNow,
                0.0,
                fs::kFSEventStreamCreateFlagFileEvents | fs::kFSEventStreamCreateFlagNoDefer,
            )
        };
        if !excluded_paths.is_empty()
            // SAFETY: `stream` was just created and is not started yet.
            && !unsafe { fs::FSEventStreamSetExclusionPaths(stream, exclude_array.as_opaque()) }
        {
            // SAFETY: never scheduled or started; releasing frees the context too.
            unsafe {
                fs::FSEventStreamInvalidate(stream);
                fs::FSEventStreamRelease(stream);
            }
            return Err(Error::generic(
                "the file-event service refused the excluded paths",
            ));
        }

        struct SendStream(fs::FSEventStreamRef);
        // SAFETY: the stream is used only by the thread it moves to.
        unsafe impl Send for SendStream {}
        struct SendRunLoop(cf::CFRetained<cf::CFRunLoop>);
        // SAFETY: CFRunLoop may move between threads (Apple's thread-safety summary).
        unsafe impl Send for SendRunLoop {}

        let stream = SendStream(stream);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("hotsheet-fsevents".into())
            .spawn(move || {
                let stream = stream;
                let stream = stream.0;
                // SAFETY: mirrors notify 8's stream thread: schedule, start, run until the
                // owner stops the loop, then stop, purge, invalidate and release.
                unsafe {
                    let Some(runloop) = cf::CFRunLoop::current() else {
                        fs::FSEventStreamInvalidate(stream);
                        fs::FSEventStreamRelease(stream);
                        let _ = ready_tx.send(Err(Error::generic("no run loop")));
                        return;
                    };
                    #[allow(
                        deprecated,
                        reason = "HS2-2HZN2C: notify 8 schedules its stream on a run loop the same way"
                    )]
                    fs::FSEventStreamScheduleWithRunLoop(
                        stream,
                        &runloop,
                        cf::kCFRunLoopDefaultMode.expect("default run-loop mode"),
                    );
                    if !fs::FSEventStreamStart(stream) {
                        fs::FSEventStreamInvalidate(stream);
                        fs::FSEventStreamRelease(stream);
                        let _ =
                            ready_tx.send(Err(Error::generic("unable to start FSEvent stream")));
                        return;
                    }
                    let _ = ready_tx.send(Ok(SendRunLoop(runloop)));
                    cf::CFRunLoop::run();
                    fs::FSEventStreamStop(stream);
                    let event_id = fs::FSEventsGetCurrentEventId();
                    let device = fs::FSEventStreamGetDeviceBeingWatched(stream);
                    fs::FSEventsPurgeEventsForDeviceUpToEventId(device, event_id);
                    fs::FSEventStreamInvalidate(stream);
                    fs::FSEventStreamRelease(stream);
                }
            })?;
        let runloop = ready_rx
            .recv()
            .map_err(|_| Error::generic("FSEvent stream thread ended"))??;
        Ok(Self {
            runloop: Some((runloop.0, thread)),
        })
    }
}

impl Drop for ExcludingStream {
    fn drop(&mut self) {
        if let Some((runloop, thread)) = self.runloop.take() {
            while !runloop.is_waiting() {
                std::thread::yield_now();
            }
            runloop.stop();
            let _ = thread.join();
        }
    }
}

unsafe extern "C-unwind" fn release_info(info: *const std::ffi::c_void) {
    // SAFETY: called once, when the stream is deallocated, with the box made in `start`.
    unsafe { drop(Box::from_raw(info as *mut StreamInfo)) };
}

unsafe extern "C-unwind" fn callback(
    _stream: fs::ConstFSEventStreamRef,
    info: *mut std::ffi::c_void,
    count: usize,
    paths: NonNull<std::ffi::c_void>,
    flags: NonNull<fs::FSEventStreamEventFlags>,
    _ids: NonNull<fs::FSEventStreamEventId>,
) {
    // SAFETY: the service passes `count` C-string paths and flags, and the `info` set up in
    // `start`, which lives until `release_info`.
    let (info, paths, flags) = unsafe {
        (
            &*(info as *const StreamInfo),
            std::slice::from_raw_parts(paths.as_ptr() as *const *const std::ffi::c_char, count),
            std::slice::from_raw_parts(flags.as_ptr(), count),
        )
    };
    let mut handler = info
        .handler
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (path, flag) in paths.iter().zip(flags) {
        // SAFETY: each entry is a NUL-terminated path owned by the service for this call.
        let path = PathBuf::from(unsafe { CStr::from_ptr(*path) }.to_string_lossy().as_ref());
        if !is_watched(&info.watched, &path) {
            continue;
        }
        for event in translate(*flag) {
            handler(Ok(event.add_path(path.clone())));
        }
    }
}

/// Whether an event at `path` belongs to a watched path: inside a recursive one, or the
/// path or a direct child of a non-recursive one.
pub(crate) fn is_watched(watched: &[(PathBuf, bool)], path: &Path) -> bool {
    watched.iter().any(|(root, recursive)| {
        path.starts_with(root) && (*recursive || path == root || path.parent() == Some(root))
    })
}

/// The events one FSEvents record stands for, as `notify` 8 translates them.
pub(crate) fn translate(flags: fs::FSEventStreamEventFlags) -> Vec<Event> {
    let has = |flag: u32| flags & flag != 0;
    let mut events = Vec::new();
    if has(fs::kFSEventStreamEventFlagHistoryDone) {
        return events;
    }
    if has(fs::kFSEventStreamEventFlagMustScanSubDirs) {
        events.push(Event::new(EventKind::Other).set_flag(Flag::Rescan));
    }
    if has(fs::kFSEventStreamEventFlagRootChanged) {
        events.push(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::From,
        ))));
    }
    let dir = has(fs::kFSEventStreamEventFlagItemIsDir);
    let file = has(fs::kFSEventStreamEventFlagItemIsFile);
    if has(fs::kFSEventStreamEventFlagItemCreated) {
        events.push(Event::new(EventKind::Create(match (dir, file) {
            (true, _) => CreateKind::Folder,
            (_, true) => CreateKind::File,
            _ => CreateKind::Any,
        })));
    }
    if has(fs::kFSEventStreamEventFlagItemRemoved) {
        events.push(Event::new(EventKind::Remove(match (dir, file) {
            (true, _) => RemoveKind::Folder,
            (_, true) => RemoveKind::File,
            _ => RemoveKind::Any,
        })));
    }
    if has(fs::kFSEventStreamEventFlagItemRenamed) {
        events.push(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::Any,
        ))));
    }
    if has(fs::kFSEventStreamEventFlagItemInodeMetaMod
        | fs::kFSEventStreamEventFlagItemFinderInfoMod
        | fs::kFSEventStreamEventFlagItemChangeOwner
        | fs::kFSEventStreamEventFlagItemXattrMod)
    {
        events.push(Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Any,
        ))));
    }
    if has(fs::kFSEventStreamEventFlagItemModified) {
        events.push(Event::new(EventKind::Modify(ModifyKind::Data(
            DataChange::Content,
        ))));
    }
    if events.is_empty() {
        events.push(Event::new(EventKind::Any));
    }
    events
}
