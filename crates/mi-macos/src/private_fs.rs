use std::ffi::CString;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateOpenMode {
    Truncate,
    Append,
    CreateNew,
}

#[cfg(unix)]
fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(unix)]
fn harden_private_dir(path: &Path) -> Result<(), String> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "private directory path contains NUL byte".to_string())?;
    let raw_fd = unsafe {
        libc::open(
            c_path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw_fd < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if (st.st_mode as u32 & libc::S_IFMT as u32) != libc::S_IFDIR as u32 {
        return Err(format!("{} is not a directory", path.display()));
    }
    if st.st_uid != current_uid() {
        return Err(format!("{} is not owned by current user", path.display()));
    }
    if (st.st_mode as u32 & 0o777) != 0o700 && unsafe { libc::fchmod(fd.as_raw_fd(), 0o700) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(())
}

#[cfg(not(unix))]
fn harden_private_dir(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
        return Err(format!("{} is not a regular directory", path.display()));
    }
    Ok(())
}

pub fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    let anchored: PathBuf = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(dir)
    };
    let components: Vec<Component<'_>> = anchored
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    if components.is_empty() {
        return Ok(());
    }
    #[cfg(unix)]
    let uid = current_uid();

    let mut cur = PathBuf::new();
    for (idx, component) in components.iter().enumerate() {
        let is_last = idx + 1 == components.len();
        match component {
            Component::RootDir | Component::Prefix(_) => {
                cur.push(component.as_os_str());
                if is_last {
                    harden_private_dir(&cur)?;
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::Normal(_) => {
                cur.push(component.as_os_str());
                loop {
                    match fs::symlink_metadata(&cur) {
                        Ok(meta) => {
                            if meta.file_type().is_symlink() {
                                if is_last {
                                    return Err(format!(
                                        "private directory must not be a symlink: {}",
                                        cur.display()
                                    ));
                                }
                                #[cfg(unix)]
                                if meta.uid() == 0 && uid != 0 {
                                    break;
                                }
                                return Err(format!(
                                    "private directory ancestor must not be a symlink: {}",
                                    cur.display()
                                ));
                            }
                            if meta.file_type().is_dir() {
                                if is_last {
                                    harden_private_dir(&cur)?;
                                } else {
                                    #[cfg(unix)]
                                    if meta.uid() != uid && meta.uid() != 0 {
                                        return Err(format!(
                                            "private directory ancestor has untrusted owner: {}",
                                            cur.display()
                                        ));
                                    }
                                }
                                break;
                            }
                            return Err(format!(
                                "private directory component is not a directory: {}",
                                cur.display()
                            ));
                        }
                        Err(err) if err.kind() == io::ErrorKind::NotFound => {
                            if !matches!(component, Component::Normal(_)) {
                                return Err(err.to_string());
                            }
                            let mut builder = DirBuilder::new();
                            #[cfg(unix)]
                            builder.mode(0o700);
                            match builder.create(&cur) {
                                Ok(()) => {
                                    harden_private_dir(&cur)?;
                                    break;
                                }
                                Err(create_err)
                                    if create_err.kind() == io::ErrorKind::AlreadyExists =>
                                {
                                    continue;
                                }
                                Err(create_err) => return Err(create_err.to_string()),
                            }
                        }
                        Err(err) => return Err(err.to_string()),
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn open_private_file(path: &Path, mode: PrivateOpenMode) -> Result<fs::File, String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            ensure_private_dir(parent)?;
        }
    }
    let mut options = OpenOptions::new();
    options.write(true);
    match mode {
        PrivateOpenMode::Truncate => {
            options.create(true).truncate(true);
        }
        PrivateOpenMode::Append => {
            options.create(true).append(true);
        }
        PrivateOpenMode::CreateNew => {
            options.create_new(true);
        }
    }
    #[cfg(unix)]
    {
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
        let file = options.open(path).map_err(|e| e.to_string())?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if !meta.file_type().is_file() {
            return Err(format!(
                "private file target is not a regular file: {}",
                path.display()
            ));
        }
        if meta.uid() != current_uid() {
            return Err(format!(
                "private file target has untrusted owner: {}",
                path.display()
            ));
        }
        if (meta.mode() & 0o777) != 0o600 {
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        let file = options.open(path).map_err(|e| e.to_string())?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if !meta.file_type().is_file() {
            return Err(format!(
                "private file target is not a regular file: {}",
                path.display()
            ));
        }
        Ok(file)
    }
}
