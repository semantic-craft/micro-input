use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

fn validate_lock_fd(fd: i32) -> io::Result<()> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let uid = unsafe { libc::getuid() };
    if (st.st_mode & libc::S_IFMT) != libc::S_IFREG || st.st_uid != uid || (st.st_mode & 0o077) != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "instance.lock failed security check",
        ));
    }
    Ok(())
}

fn read_pid_from_fd(fd: i32) -> Option<i32> {
    if unsafe { libc::lseek(fd, 0, libc::SEEK_SET) } < 0 {
        return None;
    }
    let mut buf = [0u8; 64];
    let mut total = 0usize;
    while total < buf.len() {
        let n = unsafe {
            libc::read(
                fd,
                buf[total..].as_mut_ptr().cast::<libc::c_void>(),
                buf.len() - total,
            )
        };
        if n > 0 {
            total += n as usize;
        } else if n == 0 {
            break;
        } else {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
    }
    if total == buf.len() {
        return Some(-1);
    }
    let raw = std::str::from_utf8(&buf[..total]).ok()?;
    Some(raw.trim().parse::<i32>().unwrap_or(-1))
}

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
        crate::private_fs::ensure_private_dir(document_dir)
            .map_err(|e| io::Error::new(io::ErrorKind::PermissionDenied, e))?;
        let lock_path = document_dir.join("instance.lock");
        let c_path = CString::new(lock_path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid lock path"))?;

        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | libc::O_CLOEXEC,
                0o600 as libc::c_uint,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        if let Err(err) = validate_lock_fd(fd) {
            unsafe {
                libc::close(fd);
            }
            return Err(err);
        }

        let lock_rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if lock_rc != 0 {
            let err = io::Error::last_os_error();
            unsafe {
                libc::close(fd);
            }
            if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                return Ok(None);
            }
            return Err(err);
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
        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return None;
        }

        if validate_lock_fd(fd).is_err() {
            unsafe {
                libc::close(fd);
            }
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

        let err = io::Error::last_os_error();
        let pid = if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            read_pid_from_fd(fd)
        } else {
            None
        };
        unsafe {
            libc::close(fd);
        }
        pid
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
        let dir_mode = std::fs::metadata(&doc_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        let lock_mode = std::fs::metadata(lock.lock_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(lock_mode, 0o600);
        assert_eq!(lock.lock_path(), doc_dir.join("instance.lock"));

        std::fs::write(lock.lock_path(), "not-a-pid\n").unwrap();
        assert_eq!(DocumentInstanceLock::owner_pid(&doc_dir), Some(-1));

        let oversized = format!("{}\n", "1".repeat(80));
        std::fs::write(lock.lock_path(), oversized).unwrap();
        assert_eq!(DocumentInstanceLock::owner_pid(&doc_dir), Some(-1));
    }

    #[test]
    fn instance_lock_rejects_symlink_dir_and_preserves_target_permissions() {
        let temp = TempDir::new().unwrap();
        let target_dir = temp.path().join("target");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::set_permissions(&target_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        let link_dir = temp.path().join("link-doc");
        std::os::unix::fs::symlink(&target_dir, &link_dir).unwrap();
        assert!(
            DocumentInstanceLock::acquire_with_pid(&link_dir, 999).is_err(),
            "instance lock must reject symlink document_dir"
        );
        assert_eq!(
            std::fs::metadata(&target_dir).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(
            DocumentInstanceLock::acquire_with_pid(&link_dir.join("nested"), 999).is_err(),
            "instance lock must reject user-owned intermediate symlink"
        );
    }
}
