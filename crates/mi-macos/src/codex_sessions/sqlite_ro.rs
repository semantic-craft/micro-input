use std::collections::BTreeSet;
#[cfg(test)]
use std::ffi::c_void;
use std::ffi::{c_char, c_int, c_uchar, CStr, CString};
use std::path::Path;

pub(super) const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_NULL: c_int = 5;
const SQLITE_OPEN_READONLY: c_int = 0x0000_0001;
const SQLITE_OPEN_NOFOLLOW: c_int = 0x0100_0000;
const SQLITE_TRANSIENT: isize = -1;

#[repr(C)]
pub(super) struct sqlite3 {
    _private: [u8; 0],
}

#[repr(C)]
pub(super) struct sqlite3_stmt {
    _private: [u8; 0],
}

#[link(name = "sqlite3")]
unsafe extern "C" {
    pub(super) fn sqlite3_open_v2(
        filename: *const c_char,
        pp_db: *mut *mut sqlite3,
        flags: c_int,
        z_vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close_v2(db: *mut sqlite3) -> c_int;
    fn sqlite3_busy_timeout(db: *mut sqlite3, ms: c_int) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut sqlite3,
        z_sql: *const c_char,
        n_byte: c_int,
        pp_stmt: *mut *mut sqlite3_stmt,
        pz_tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_step(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_bind_text(
        stmt: *mut sqlite3_stmt,
        idx: c_int,
        val: *const c_char,
        n_bytes: c_int,
        destructor: isize,
    ) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut sqlite3_stmt, idx: c_int, val: i64) -> c_int;
    fn sqlite3_column_type(stmt: *mut sqlite3_stmt, i_col: c_int) -> c_int;
    fn sqlite3_column_text(stmt: *mut sqlite3_stmt, i_col: c_int) -> *const c_uchar;
    fn sqlite3_column_bytes(stmt: *mut sqlite3_stmt, i_col: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut sqlite3_stmt, i_col: c_int) -> i64;
    fn sqlite3_column_double(stmt: *mut sqlite3_stmt, i_col: c_int) -> f64;
    fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
    #[cfg(test)]
    pub(super) fn sqlite3_exec(
        db: *mut sqlite3,
        sql: *const c_char,
        callback: Option<
            unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int,
        >,
        arg: *mut c_void,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    #[cfg(test)]
    pub(super) fn sqlite3_free(ptr: *mut c_void);
}

pub(super) struct SqliteDb {
    pub(super) ptr: *mut sqlite3,
}

impl Drop for SqliteDb {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                sqlite3_close_v2(self.ptr);
            }
            self.ptr = std::ptr::null_mut();
        }
    }
}

impl SqliteDb {
    pub(super) fn open_readonly(path: &Path) -> Result<Self, String> {
        let redacted = crate::redact_home_path(&path.display().to_string());
        let open_path = resolve_nofollow_sqlite_path(path, &redacted)?;
        let meta = match std::fs::symlink_metadata(&open_path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(format!("blocked:codex_sqlite_missing:{redacted}"));
            }
            Err(_) => return Err(format!("blocked:codex_sqlite_unsafe:{redacted}")),
        };
        if meta.file_type().is_symlink() || !meta.file_type().is_file() {
            return Err(format!("blocked:codex_sqlite_unsafe:{redacted}"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.uid() != unsafe { libc::getuid() } {
                return Err(format!("blocked:codex_sqlite_owner_mismatch:{redacted}"));
            }
        }
        let raw_path = open_path
            .to_str()
            .ok_or_else(|| "blocked:codex_sqlite_non_utf8_path".to_owned())?;
        let c_path =
            CString::new(raw_path).map_err(|e| format!("blocked:codex_sqlite_path:{e}"))?;
        let mut raw_db: *mut sqlite3 = std::ptr::null_mut();
        let rc = unsafe {
            sqlite3_open_v2(
                c_path.as_ptr(),
                &mut raw_db,
                SQLITE_OPEN_READONLY | SQLITE_OPEN_NOFOLLOW,
                std::ptr::null(),
            )
        };
        let db = Self { ptr: raw_db };
        if rc != SQLITE_OK {
            return Err(format!(
                "blocked:codex_sqlite_open:{redacted}:{rc}:{}",
                db.last_error()
            ));
        }
        db.check_rc("busy_timeout", unsafe { sqlite3_busy_timeout(db.ptr, 250) })?;
        Ok(db)
    }

    pub(super) fn query_text_set(&self, sql: &str) -> Result<BTreeSet<String>, String> {
        let mut stmt = self.prepare(sql)?;
        let mut set = BTreeSet::new();
        while stmt.step()? {
            let value = stmt.col_text(0)?;
            if !value.is_empty() {
                set.insert(value);
            }
        }
        Ok(set)
    }

    pub(super) fn query_optional_text_pairs(
        &self,
        sql: &str,
    ) -> Result<Vec<(String, String)>, String> {
        let Ok(mut stmt) = self.prepare(sql) else {
            return Ok(Vec::new());
        };
        let mut pairs = Vec::new();
        while stmt.step()? {
            let left = stmt.col_text(0)?;
            let right = stmt.col_text(1)?;
            let left = left.trim();
            let right = right.trim();
            if !left.is_empty() && !right.is_empty() {
                pairs.push((left.to_owned(), right.to_owned()));
            }
        }
        Ok(pairs)
    }

    fn check_rc(&self, op: &str, rc: c_int) -> Result<(), String> {
        if rc == SQLITE_OK {
            Ok(())
        } else {
            Err(format!(
                "blocked:codex_sqlite_{op}:{rc}:{}",
                self.last_error()
            ))
        }
    }

    pub(super) fn last_error(&self) -> String {
        if self.ptr.is_null() {
            return "null sqlite handle".to_owned();
        }
        let msg = unsafe { sqlite3_errmsg(self.ptr) };
        if msg.is_null() {
            "unknown sqlite error".to_owned()
        } else {
            unsafe { CStr::from_ptr(msg) }
                .to_string_lossy()
                .into_owned()
        }
    }

    pub(super) fn prepare(&self, sql: &str) -> Result<SqliteStmt<'_>, String> {
        let c_sql = CString::new(sql).map_err(|e| format!("blocked:codex_sql_invalid:{e}"))?;
        let mut raw_stmt: *mut sqlite3_stmt = std::ptr::null_mut();
        let rc = unsafe {
            sqlite3_prepare_v2(
                self.ptr,
                c_sql.as_ptr(),
                sql.len() as c_int,
                &mut raw_stmt,
                std::ptr::null_mut(),
            )
        };
        self.check_rc("prepare", rc)?;
        if raw_stmt.is_null() {
            return Err(format!(
                "blocked:codex_sqlite_prepare:{rc}:{}",
                self.last_error()
            ));
        }
        Ok(SqliteStmt {
            db: self,
            ptr: raw_stmt,
        })
    }
}

pub(super) struct SqliteStmt<'a> {
    db: &'a SqliteDb,
    ptr: *mut sqlite3_stmt,
}

impl Drop for SqliteStmt<'_> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { sqlite3_finalize(self.ptr) };
            self.ptr = std::ptr::null_mut();
        }
    }
}

pub(super) fn checked_sqlite_text_len(len: usize) -> Result<c_int, String> {
    c_int::try_from(len).map_err(|_| "sqlite3 text binding exceeds i32 length limit".to_string())
}

impl SqliteStmt<'_> {
    pub(super) fn bind_text(&mut self, index: c_int, value: &str) -> Result<(), String> {
        let len = checked_sqlite_text_len(value.len())?;
        let rc = unsafe {
            sqlite3_bind_text(
                self.ptr,
                index,
                value.as_ptr().cast::<c_char>(),
                len,
                SQLITE_TRANSIENT,
            )
        };
        self.db.check_rc("bind_text", rc)
    }

    pub(super) fn bind_int64(&mut self, index: c_int, value: i64) -> Result<(), String> {
        self.db.check_rc("bind_int64", unsafe {
            sqlite3_bind_int64(self.ptr, index, value)
        })
    }

    pub(super) fn step(&mut self) -> Result<bool, String> {
        match unsafe { sqlite3_step(self.ptr) } {
            SQLITE_ROW => Ok(true),
            SQLITE_DONE => Ok(false),
            other => Err(self.db.check_rc("step", other).unwrap_err()),
        }
    }

    pub(super) fn col_opt_text(&self, col: c_int) -> Result<Option<String>, String> {
        if unsafe { sqlite3_column_type(self.ptr, col) } == SQLITE_NULL {
            return Ok(None);
        }
        let ptr = unsafe { sqlite3_column_text(self.ptr, col) };
        let len = unsafe { sqlite3_column_bytes(self.ptr, col) };
        if ptr.is_null() || len < 0 {
            return Ok(Some(String::new()));
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        std::str::from_utf8(bytes)
            .map(|s| Some(s.to_owned()))
            .map_err(|e| format!("blocked:codex_sqlite_utf8:{e}"))
    }

    pub(super) fn col_bounded_text(
        &self,
        col: c_int,
        max_bytes: usize,
    ) -> Result<Option<String>, String> {
        if unsafe { sqlite3_column_type(self.ptr, col) } == SQLITE_NULL {
            return Ok(None);
        }
        let len = unsafe { sqlite3_column_bytes(self.ptr, col) };
        if len < 0 || len as usize > max_bytes {
            return Ok(None);
        }
        let ptr = unsafe { sqlite3_column_text(self.ptr, col) };
        let len = unsafe { sqlite3_column_bytes(self.ptr, col) };
        if ptr.is_null() || len <= 0 {
            return Ok(Some(String::new()));
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        std::str::from_utf8(bytes)
            .map(|s| Some(s.to_owned()))
            .map_err(|e| format!("blocked:codex_sqlite_utf8:{e}"))
    }

    pub(super) fn col_text(&self, col: c_int) -> Result<String, String> {
        Ok(self.col_opt_text(col)?.unwrap_or_default())
    }

    pub(super) fn col_int64(&self, col: c_int) -> i64 {
        unsafe { sqlite3_column_int64(self.ptr, col) }
    }

    pub(super) fn col_double(&self, col: c_int) -> f64 {
        unsafe { sqlite3_column_double(self.ptr, col) }
    }
}

fn map_private_dir_error(
    err: crate::private_fs::PrivateDirError,
    prefix: &str,
    redacted: &str,
) -> String {
    let tag = match err {
        crate::private_fs::PrivateDirError::Missing => "missing",
        crate::private_fs::PrivateDirError::Unsafe => "unsafe",
        crate::private_fs::PrivateDirError::OwnerMismatch => "owner_mismatch",
    };
    format!("{prefix}_{tag}:{redacted}")
}

fn resolve_nofollow_sqlite_path(path: &Path, redacted: &str) -> Result<std::path::PathBuf, String> {
    let Some(file_name) = path.file_name() else {
        return Err(format!("blocked:codex_sqlite_unsafe:{redacted}"));
    };
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let verified_parent = crate::private_fs::verify_existing_private_dir(parent)
        .map_err(|err| map_private_dir_error(err, "blocked:codex_sqlite", redacted))?;
    Ok(verified_parent.join(file_name))
}

pub(super) fn validate_codex_dir(dir: &Path) -> Result<(), String> {
    let redacted = crate::redact_home_path(&dir.display().to_string());
    crate::private_fs::verify_existing_private_dir(dir)
        .map(|_| ())
        .map_err(|err| map_private_dir_error(err, "blocked:codex_home", &redacted))
}
