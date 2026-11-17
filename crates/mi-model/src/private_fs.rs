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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateDirError {
    Missing,
    Unsafe,
    OwnerMismatch,
}

#[cfg(unix)]
fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(unix)]
enum OpenDirFailure {
    NulByte,
    Io(io::Error),
    NotDir,
    OwnerMismatch,
}

#[cfg(unix)]
fn open_verified_dir_fd(path: &Path, uid: u32) -> Result<(OwnedFd, libc::stat), OpenDirFailure> {
    let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| OpenDirFailure::NulByte)?;
    let raw_fd = unsafe {
        libc::open(
            c_path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw_fd < 0 {
        return Err(OpenDirFailure::Io(io::Error::last_os_error()));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
        return Err(OpenDirFailure::Io(io::Error::last_os_error()));
    }
    if (st.st_mode as u32 & libc::S_IFMT as u32) != libc::S_IFDIR as u32 {
        return Err(OpenDirFailure::NotDir);
    }
    if st.st_uid != uid {
        return Err(OpenDirFailure::OwnerMismatch);
    }
    Ok((fd, st))
}

#[cfg(unix)]
fn harden_private_dir(path: &Path) -> Result<(), String> {
    let (fd, st) = open_verified_dir_fd(path, current_uid()).map_err(|err| match err {
        OpenDirFailure::NulByte => "private directory path contains NUL byte".to_string(),
        OpenDirFailure::Io(io_err) => io_err.to_string(),
        OpenDirFailure::NotDir => format!("{} is not a directory", path.display()),
        OpenDirFailure::OwnerMismatch => format!("{} is not owned by current user", path.display()),
    })?;
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

fn verify_dir_component_meta(
    cur: &Path,
    is_last: bool,
    #[cfg_attr(not(unix), allow(unused_variables))] uid: u32,
) -> Result<Option<PathBuf>, PrivateDirError> {
    let meta = match fs::symlink_metadata(cur) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(PrivateDirError::Missing);
        }
        Err(_) => return Err(PrivateDirError::Unsafe),
    };
    if meta.file_type().is_symlink() {
        if !is_last {
            #[cfg(unix)]
            if meta.uid() == 0 {
                let canonical = fs::canonicalize(cur).map_err(|_| PrivateDirError::Unsafe)?;
                return Ok(Some(canonical));
            }
        }
        return Err(PrivateDirError::Unsafe);
    }
    if !meta.file_type().is_dir() {
        return Err(PrivateDirError::Unsafe);
    }
    #[cfg(unix)]
    {
        if (is_last && meta.uid() != uid) || (!is_last && meta.uid() != uid && meta.uid() != 0) {
            return Err(PrivateDirError::OwnerMismatch);
        }
        let mode = meta.mode();
        let mode_ok = if is_last {
            (mode & 0o022) == 0
        } else {
            (mode & 0o022) == 0 || (mode & 0o1000) != 0
        };
        if !mode_ok {
            return Err(PrivateDirError::Unsafe);
        }
    }
    Ok(None)
}

pub fn verify_existing_private_dir(dir: &Path) -> Result<PathBuf, PrivateDirError> {
    if dir.as_os_str().is_empty() {
        return Err(PrivateDirError::Missing);
    }
    let anchored = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| PrivateDirError::Unsafe)?
            .join(dir)
    };
    let components: Vec<Component<'_>> = anchored
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    if components.is_empty() {
        return Err(PrivateDirError::Missing);
    }
    #[cfg(unix)]
    let uid = current_uid();
    #[cfg(not(unix))]
    let uid = 0u32;
    let mut cur = PathBuf::new();
    for (idx, component) in components.iter().enumerate() {
        let is_last = idx + 1 == components.len();
        match component {
            Component::RootDir | Component::Prefix(_) => {
                cur.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if cur.parent().is_none() || !cur.pop() {
                    return Err(PrivateDirError::Unsafe);
                }
                if is_last {
                    let _ = verify_dir_component_meta(&cur, true, uid)?;
                }
            }
            Component::Normal(_) => {
                cur.push(component.as_os_str());
                if let Some(canonical) = verify_dir_component_meta(&cur, is_last, uid)? {
                    let _ = verify_dir_component_meta(&canonical, is_last, uid)?;
                    cur = canonical;
                }
            }
        }
    }
    #[cfg(unix)]
    {
        let (_fd, st) = open_verified_dir_fd(&cur, uid).map_err(|err| match err {
            OpenDirFailure::Io(io_err) if io_err.kind() == io::ErrorKind::NotFound => {
                PrivateDirError::Missing
            }
            OpenDirFailure::OwnerMismatch => PrivateDirError::OwnerMismatch,
            OpenDirFailure::NulByte | OpenDirFailure::Io(_) | OpenDirFailure::NotDir => {
                PrivateDirError::Unsafe
            }
        })?;
        if (st.st_mode as u32 & 0o022) != 0 {
            return Err(PrivateDirError::Unsafe);
        }
    }
    Ok(cur)
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

pub fn redact_home_path_with_home(raw: &str, home: Option<&str>) -> String {
    if let Some(home_dir) = home
        .map(|h| h.trim_end_matches('/'))
        .filter(|h| !h.is_empty() && *h != "/")
    {
        if raw == home_dir {
            return "~".to_string();
        }
        if let Some(rest) = raw.strip_prefix(home_dir).filter(|r| r.starts_with('/')) {
            return format!("~{rest}");
        }
    }
    if let Some(after_users) = raw.strip_prefix("/Users/") {
        let (user, rest) = match after_users.split_once('/') {
            Some((u, r)) => (u, Some(r)),
            None => (after_users, None),
        };
        if !user.is_empty() && user != "Shared" {
            return match rest {
                Some(r) => format!("/Users/[redacted]/{r}"),
                None => "/Users/[redacted]".to_string(),
            };
        }
    }
    raw.to_string()
}

pub fn redact_home_path(raw: &str) -> String {
    redact_home_path_with_home(raw, std::env::var("HOME").ok().as_deref())
}

pub fn open_verified_readonly_file(path: &Path) -> Result<fs::File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
        let file = options.open(path).map_err(|e| e.to_string())?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if !meta.file_type().is_file() {
            return Err(format!(
                "verified file target is not a regular file: {}",
                redact_home_path(&path.display().to_string())
            ));
        }
        if meta.uid() != current_uid() {
            return Err(format!(
                "verified file target has untrusted owner: {}",
                redact_home_path(&path.display().to_string())
            ));
        }
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() || !meta.file_type().is_file() {
            return Err(format!(
                "verified file target is not a regular file: {}",
                redact_home_path(&path.display().to_string())
            ));
        }
        let file = options.open(path).map_err(|e| e.to_string())?;
        let fmeta = file.metadata().map_err(|e| e.to_string())?;
        if !fmeta.file_type().is_file() {
            return Err(format!(
                "verified file target is not a regular file: {}",
                redact_home_path(&path.display().to_string())
            ));
        }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_home_path_collapses_home_and_non_shared_users() {
        assert_eq!(
            redact_home_path_with_home("/Users/example", Some("/Users/example")),
            "~"
        );
        assert_eq!(
            redact_home_path_with_home("/Users/example/work/app", Some("/Users/example/")),
            "~/work/app"
        );
        let sibling_home = ["/Users/example", "2/work"].concat();
        assert_eq!(
            redact_home_path_with_home(&sibling_home, Some("/Users/example")),
            "/Users/[redacted]/work"
        );
        assert_eq!(
            redact_home_path_with_home("/Users/test/work", Some("/Users/example")),
            "/Users/[redacted]/work"
        );
        assert_eq!(
            redact_home_path_with_home("/Users/Shared/repo", Some("/Users/example")),
            "/Users/Shared/repo"
        );
        assert_eq!(
            redact_home_path_with_home("/tmp/work", Some("/Users/example")),
            "/tmp/work"
        );
    }

    #[test]
    fn verify_existing_private_dir_checks_ancestors_preserves_perms_and_rejects_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let dir_755 = temp.path().join("existing-codex");
        fs::create_dir_all(&dir_755).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&dir_755, fs::Permissions::from_mode(0o755)).unwrap();

        let verified = verify_existing_private_dir(&dir_755).unwrap();
        assert_eq!(verified, fs::canonicalize(&dir_755).unwrap());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&dir_755).unwrap().permissions().mode() & 0o777,
            0o755
        );

        assert_eq!(
            verify_existing_private_dir(&temp.path().join("missing")),
            Err(PrivateDirError::Missing)
        );
        assert_eq!(
            verify_existing_private_dir(Path::new("")),
            Err(PrivateDirError::Missing)
        );

        let file_path = temp.path().join("not-a-dir");
        fs::write(&file_path, b"x").unwrap();
        assert_eq!(
            verify_existing_private_dir(&file_path),
            Err(PrivateDirError::Unsafe)
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let leaf_link = temp.path().join("leaf-link");
            symlink(&dir_755, &leaf_link).unwrap();
            assert_eq!(
                verify_existing_private_dir(&leaf_link),
                Err(PrivateDirError::Unsafe)
            );

            let nested_real = dir_755.join("nested");
            fs::create_dir_all(&nested_real).unwrap();
            let ancestor_link = temp.path().join("ancestor-link");
            symlink(&dir_755, &ancestor_link).unwrap();
            assert_eq!(
                verify_existing_private_dir(&ancestor_link.join("nested")),
                Err(PrivateDirError::Unsafe)
            );

            if current_uid() != 0 {
                assert_eq!(
                    verify_existing_private_dir(Path::new("/")),
                    Err(PrivateDirError::OwnerMismatch)
                );
            }

            let writable_leaf = temp.path().join("writable-leaf");
            fs::create_dir_all(&writable_leaf).unwrap();
            fs::set_permissions(&writable_leaf, fs::Permissions::from_mode(0o777)).unwrap();
            assert_eq!(
                verify_existing_private_dir(&writable_leaf),
                Err(PrivateDirError::Unsafe)
            );
            fs::set_permissions(&writable_leaf, fs::Permissions::from_mode(0o775)).unwrap();
            assert_eq!(
                verify_existing_private_dir(&writable_leaf),
                Err(PrivateDirError::Unsafe)
            );

            let ancestor = temp.path().join("ancestor");
            let safe_child = ancestor.join("child");
            fs::create_dir_all(&safe_child).unwrap();
            fs::set_permissions(&safe_child, fs::Permissions::from_mode(0o700)).unwrap();

            fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o777)).unwrap();
            assert_eq!(
                verify_existing_private_dir(&safe_child),
                Err(PrivateDirError::Unsafe)
            );

            fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o1777)).unwrap();
            assert_eq!(
                verify_existing_private_dir(&safe_child).unwrap(),
                fs::canonicalize(&safe_child).unwrap()
            );

            fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o700)).unwrap();
            let sibling_child = ancestor.join("sibling");
            fs::create_dir_all(&sibling_child).unwrap();
            fs::set_permissions(&sibling_child, fs::Permissions::from_mode(0o700)).unwrap();

            let normalized_mid =
                verify_existing_private_dir(&safe_child.join("..").join("sibling")).unwrap();
            assert_eq!(normalized_mid, fs::canonicalize(&sibling_child).unwrap());
            assert!(!normalized_mid.to_string_lossy().contains(".."));

            let normalized_end = verify_existing_private_dir(&safe_child.join("..")).unwrap();
            assert_eq!(normalized_end, fs::canonicalize(&ancestor).unwrap());
            assert!(!normalized_end.to_string_lossy().contains(".."));

            assert_eq!(
                verify_existing_private_dir(Path::new("/..")),
                Err(PrivateDirError::Unsafe)
            );
        }
    }
}
