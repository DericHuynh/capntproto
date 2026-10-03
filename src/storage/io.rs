//! File operations with deterministic fault injection compiled only for unit tests.
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::Path,
};

pub(super) fn directory(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // CreateFile requires backup semantics for directories; the subsequent
        // FlushFileBuffers (File::sync_all) also requires GENERIC_WRITE access.
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        options.write(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    options.open(path)
}

pub(super) fn persist(temporary: tempfile::NamedTempFile, path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        // Clear FILE_ATTRIBUTE_TEMPORARY before replacement, preserving the
        // original locked handle. Rust's rename has a FileRenameInfoEx/POSIX
        // fallback for destinations held open by immutable mmap snapshots;
        // tempfile::persist currently only tries MoveFileExW.
        let (file, source) = temporary.keep().map_err(|error| error.error)?;
        let mut cleanup = tempfile::TempPath::try_from_path(source)?;
        std::fs::rename(&cleanup, path)?;
        cleanup.disable_cleanup(true);
        Ok(file)
    }
    #[cfg(not(windows))]
    temporary.persist(path).map_err(|error| error.error)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Point {
    AppendWrite,
    Appended,
    AppendSync,
    AppendSynced,
    RecoverySync,
    RecoveryDirectorySync,
    Recovered,
    CompactWrite,
    CompactSync,
    CompactSynced,
    CompactRename,
    CompactRenamed,
    CompactDirectorySync,
    CompactComplete,
}

pub(super) fn point(at: Point) -> io::Result<()> {
    #[cfg(test)]
    faults::visit(at, 0)?;
    #[cfg(not(test))]
    let _ = at;
    Ok(())
}

pub(super) fn sync(file: &File, at: Point) -> io::Result<()> {
    point(at)?;
    file.sync_all()
}

pub(super) struct StorageWriter<'a> {
    file: &'a File,
    at: Point,
}
impl<'a> StorageWriter<'a> {
    pub(super) fn new(file: &'a File, at: Point) -> Self {
        Self { file, at }
    }
}
impl Write for StorageWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        #[cfg(test)]
        let size = faults::visit(self.at, bytes.len())?;
        #[cfg(not(test))]
        let size = {
            let _ = self.at;
            bytes.len()
        };
        self.file.write(&bytes[..size])
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
pub(super) mod faults {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque};

    #[derive(Clone, Copy)]
    pub(in crate::storage) enum Action {
        Short(usize),
        Error(i32),
        Crash,
    }
    thread_local! {
        static PLAN: RefCell<VecDeque<(Point, Action)>> = const { RefCell::new(VecDeque::new()) };
        static VISITS: RefCell<Vec<Point>> = const { RefCell::new(Vec::new()) };
    }
    pub(in crate::storage) struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            PLAN.with(|p| p.borrow_mut().clear());
        }
    }
    pub(in crate::storage) fn install(steps: impl IntoIterator<Item = (Point, Action)>) -> Guard {
        PLAN.with(|p| {
            assert!(p.borrow().is_empty());
            *p.borrow_mut() = steps.into_iter().collect();
        });
        VISITS.with(|p| p.borrow_mut().clear());
        Guard
    }
    pub(in crate::storage) fn visits() -> Vec<Point> {
        VISITS.with(|p| p.borrow().clone())
    }
    pub(super) fn visit(at: Point, size: usize) -> io::Result<usize> {
        VISITS.with(|p| p.borrow_mut().push(at));
        let action = PLAN.with(|p| {
            let mut plan = p.borrow_mut();
            if plan.front().is_some_and(|(point, _)| *point == at) {
                plan.pop_front().map(|(_, action)| action)
            } else {
                None
            }
        });
        match action {
            Some(Action::Short(n)) => Ok(n.min(size)),
            Some(Action::Error(errno)) => Err(io::Error::from_raw_os_error(errno)),
            Some(Action::Crash) => std::process::exit(86),
            None => Ok(size),
        }
    }
}
