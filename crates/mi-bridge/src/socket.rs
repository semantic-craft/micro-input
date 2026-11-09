use std::ffi::CString;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug)]
pub(crate) enum SocketError {
    Unavailable,
    InvalidPeer,
    InvalidReply,
}

pub(crate) struct LocalSocket {
    pub path: PathBuf,
    pub request_limit: usize,
    pub response_limit: usize,
    pub timeout: Duration,
}

impl LocalSocket {
    pub fn exchange_blocking(&self, request_bytes: &[u8]) -> Result<Vec<u8>, SocketError> {
        let c_path =
            CString::new(self.path.as_os_str().as_bytes()).map_err(|_| SocketError::InvalidPeer)?;
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::lstat(c_path.as_ptr(), &mut st) };
        let uid = unsafe { libc::getuid() };
        if rc != 0
            || st.st_uid != uid
            || (st.st_mode & libc::S_IFMT) != libc::S_IFSOCK
            || (st.st_mode & 0o077) != 0
        {
            return Err(SocketError::InvalidPeer);
        }
        if request_bytes.len() > self.request_limit {
            return Err(SocketError::InvalidReply);
        }
        let mut stream = UnixStream::connect(&self.path).map_err(|_| SocketError::Unavailable)?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|_| SocketError::Unavailable)?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|_| SocketError::Unavailable)?;
        let mut peer_uid: libc::uid_t = 0;
        let mut peer_gid: libc::gid_t = 0;
        let peer_rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut peer_uid, &mut peer_gid) };
        if peer_rc != 0 || peer_uid != uid {
            return Err(SocketError::InvalidPeer);
        }
        let mut frame = Vec::with_capacity(request_bytes.len() + 1);
        frame.extend_from_slice(request_bytes);
        frame.push(b'\n');
        stream
            .write_all(&frame)
            .map_err(|_| SocketError::Unavailable)?;
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        while response.len() <= self.response_limit {
            let n = stream
                .read(&mut buf)
                .map_err(|_| SocketError::Unavailable)?;
            if n == 0 {
                return Err(SocketError::Unavailable);
            }
            response.extend_from_slice(&buf[..n]);
            if let Some(nl) = response.iter().position(|&b| b == b'\n') {
                if nl > self.response_limit || response.len() != nl + 1 {
                    return Err(SocketError::InvalidReply);
                }
                response.truncate(nl);
                return Ok(response);
            }
        }
        Err(SocketError::InvalidReply)
    }
}
