use std::ffi::CString;
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct DocumentInstanceLock {
    lock_path: PathBuf,
    fd: Option<i32>,
}

impl DocumentInstanceLock {
    pub fn acquire(document_dir: &Path) -> io::Result<Option<Self>> {
        Self::acquire_with_pid(document_dir, std::process::id() as i32)
    }

    pub fn acquire_with_pid(document_dir: &Path, pid: i32) -> io::Result<Option<Self>> {
        if !document_dir.exists() {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(document_dir)?;
        }
        let lock_path = document_dir.join("instance.lock");
        let c_path = CString::new(lock_path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid lock path"))?;

        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC,
                0o600 as libc::c_uint,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let lock_rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if lock_rc != 0 {
            unsafe {
                libc::close(fd);
            }
            return Ok(None);
        }

        let payload = format!("{pid}\n");
        let write_ok = unsafe {
            libc::ftruncate(fd, 0) == 0
                && libc::lseek(fd, 0, libc::SEEK_SET) >= 0
                && libc::write(fd, payload.as_ptr() as *const libc::c_void, payload.len())
                    == payload.len() as isize
                && libc::fsync(fd) == 0
        };
        if !write_ok {
            let err = io::Error::last_os_error();
            unsafe {
                libc::flock(fd, libc::LOCK_UN);
                libc::close(fd);
            }
            return Err(err);
        }

        Ok(Some(Self {
            lock_path,
            fd: Some(fd),
        }))
    }

    pub fn owner_pid(document_dir: &Path) -> Option<i32> {
        let lock_path = document_dir.join("instance.lock");
        let c_path = CString::new(lock_path.as_os_str().as_bytes()).ok()?;
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
        if fd < 0 {
            return None;
        }

        let probe_rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if probe_rc == 0 {
            unsafe {
                libc::flock(fd, libc::LOCK_UN);
                libc::close(fd);
            }
            return None;
        }

        unsafe {
            libc::close(fd);
        }
        let raw = fs::read_to_string(&lock_path).ok()?;
        Some(raw.trim().parse::<i32>().unwrap_or(-1))
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    pub fn is_held(&self) -> bool {
        self.fd.is_some()
    }

    pub fn release(&mut self) {
        if let Some(fd) = self.fd.take() {
            unsafe {
                libc::flock(fd, libc::LOCK_UN);
                libc::close(fd);
            }
        }
    }
}

impl Drop for DocumentInstanceLock {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn instance_lock_creates_private_directory_and_reports_unparseable_pid() {
        let temp = TempDir::new().unwrap();
        let doc_dir = temp.path().join("nested/doc-dir");

        let lock = DocumentInstanceLock::acquire_with_pid(&doc_dir, 12345)
            .unwrap()
            .unwrap();
        let dir_mode = fs::metadata(&doc_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(lock.lock_path(), doc_dir.join("instance.lock"));

        fs::write(lock.lock_path(), "not-a-pid\n").unwrap();
        assert_eq!(DocumentInstanceLock::owner_pid(&doc_dir), Some(-1));
    }
}
