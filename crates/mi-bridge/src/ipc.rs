use std::ffi::CString;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UnixTransportError {
    #[error("Unix JSON socket unavailable")]
    Unavailable,
    #[error("Unix JSON peer failed security check")]
    InvalidPeer,
    #[error("Unix JSON reply invalid or exceeded limits")]
    InvalidReply,
}

#[derive(Debug)]
struct PathLockGuard {
    slot: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}

impl Drop for PathLockGuard {
    fn drop(&mut self) {
        let mut busy = self.slot.0.lock().unwrap_or_else(|e| e.into_inner());
        *busy = false;
        self.slot.1.notify_one();
    }
}

#[derive(Debug, Clone)]
pub struct UnixJsonTransport {
    pub path: PathBuf,
    pub request_limit: usize,
    pub response_limit: usize,
    pub timeout: Duration,
}

impl UnixJsonTransport {
    fn lock_path(&self) -> PathLockGuard {
        static PATH_LOCKS: std::sync::OnceLock<
            std::sync::Mutex<
                std::collections::BTreeMap<
                    PathBuf,
                    Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
                >,
            >,
        > = std::sync::OnceLock::new();
        let slot = {
            let mut map = PATH_LOCKS
                .get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()))
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            map.entry(self.path.clone())
                .or_insert_with(|| {
                    Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()))
                })
                .clone()
        };
        let mut busy = slot.0.lock().unwrap_or_else(|e| e.into_inner());
        while *busy {
            busy = slot.1.wait(busy).unwrap_or_else(|e| e.into_inner());
        }
        *busy = true;
        drop(busy);
        PathLockGuard { slot }
    }

    pub fn exchange_blocking(&self, request_bytes: &[u8]) -> Result<Vec<u8>, UnixTransportError> {
        let _guard = self.lock_path();
        self.exchange_blocking_locked(request_bytes)
    }

    fn exchange_blocking_locked(
        &self,
        request_bytes: &[u8],
    ) -> Result<Vec<u8>, UnixTransportError> {
        let c_path = CString::new(self.path.as_os_str().as_bytes())
            .map_err(|_| UnixTransportError::InvalidPeer)?;
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::lstat(c_path.as_ptr(), &mut st) };
        let uid = unsafe { libc::getuid() };
        if rc != 0
            || st.st_uid != uid
            || (st.st_mode & libc::S_IFMT) != libc::S_IFSOCK
            || (st.st_mode & 0o077) != 0
        {
            return Err(UnixTransportError::InvalidPeer);
        }
        if request_bytes.len() > self.request_limit {
            return Err(UnixTransportError::InvalidReply);
        }
        let mut stream =
            UnixStream::connect(&self.path).map_err(|_| UnixTransportError::Unavailable)?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|_| UnixTransportError::Unavailable)?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|_| UnixTransportError::Unavailable)?;
        let mut peer_uid: libc::uid_t = 0;
        let mut peer_gid: libc::gid_t = 0;
        let peer_rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut peer_uid, &mut peer_gid) };
        if peer_rc != 0 || peer_uid != uid {
            return Err(UnixTransportError::InvalidPeer);
        }
        let mut frame = Vec::with_capacity(request_bytes.len() + 1);
        frame.extend_from_slice(request_bytes);
        frame.push(b'\n');
        stream
            .write_all(&frame)
            .map_err(|_| UnixTransportError::Unavailable)?;
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        while response.len() <= self.response_limit {
            let n = stream
                .read(&mut buf)
                .map_err(|_| UnixTransportError::Unavailable)?;
            if n == 0 {
                return Err(UnixTransportError::Unavailable);
            }
            response.extend_from_slice(&buf[..n]);
            if let Some(nl) = response.iter().position(|&b| b == b'\n') {
                if nl > self.response_limit || response.len() != nl + 1 {
                    return Err(UnixTransportError::InvalidReply);
                }
                response.truncate(nl);
                return Ok(response);
            }
        }
        Err(UnixTransportError::InvalidReply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};

    fn transport(path: PathBuf) -> UnixJsonTransport {
        UnixJsonTransport {
            path,
            request_limit: 8,
            response_limit: 8,
            timeout: Duration::from_millis(20),
        }
    }

    #[test]
    fn rejects_public_socket_regular_file_and_oversized_request_before_connecting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rpc.sock");
        std::fs::write(&path, b"{}").unwrap();
        assert_eq!(
            transport(path.clone()).exchange_blocking(b"{}"),
            Err(UnixTransportError::InvalidPeer)
        );
        std::fs::remove_file(&path).unwrap();
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert_eq!(
            transport(path.clone()).exchange_blocking(b"{}"),
            Err(UnixTransportError::InvalidPeer)
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            transport(path).exchange_blocking(b"123456789"),
            Err(UnixTransportError::InvalidReply)
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn bounds_reply_frame_and_timeout_and_accepts_private_peer() {
        for (reply, expected) in [
            (Some(b"{}\n".as_slice()), Ok(b"{}".to_vec())),
            (
                Some(b"123456789\n".as_slice()),
                Err(UnixTransportError::InvalidReply),
            ),
            (
                Some(b"{}\n{}\n".as_slice()),
                Err(UnixTransportError::InvalidReply),
            ),
            (None, Err(UnixTransportError::Unavailable)),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("rpc.sock");
            let listener = UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::thread::scope(|scope| {
                scope.spawn(move || {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = [0; 3];
                    stream.read_exact(&mut request).unwrap();
                    assert_eq!(&request, b"{}\n");
                    if let Some(reply) = reply {
                        stream.write_all(reply).unwrap();
                    } else {
                        std::thread::sleep(Duration::from_millis(80));
                    }
                });
                assert_eq!(transport(path).exchange_blocking(b"{}"), expected);
            });
        }
    }
}
