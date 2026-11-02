//! R-E08: bounded local storage, conflict detection, atomic replacement. No device access.
use crate::draft::Draft;
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};
const LIMIT: u64 = 4 * 1024 * 1024;
#[derive(Default)]
pub struct DocumentStore {
    pub draft: Draft,
    path: Option<PathBuf>,
    expected: Option<Vec<u8>>,
    blocked: bool,
}
fn read(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    let f = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut b = Vec::new();
    f.take(LIMIT + 1)
        .read_to_end(&mut b)
        .map_err(|e| e.to_string())?;
    if b.len() as u64 > LIMIT {
        return Err("本地文档超过 4 MiB；原文件保留".into());
    }
    Ok(Some(b))
}
impl DocumentStore {
    pub fn open(&mut self, path: PathBuf) -> Result<(), String> {
        self.blocked = true;
        let bytes = read(&path)?;
        let draft = match &bytes {
            Some(b) => Draft::from_bytes(b)?,
            None => Draft::default(),
        };
        self.draft = draft;
        self.path = Some(path);
        self.expected = bytes;
        self.blocked = false;
        Ok(())
    }
    pub fn update(
        &mut self,
        edit: impl FnOnce(&mut Draft) -> Result<(), String>,
    ) -> Result<(), String> {
        if self.blocked {
            return Err("本地文档未成功载入，已停止保存；原文件保留".into());
        }
        let mut next = self.draft.clone();
        edit(&mut next)?;
        if let Some(path) = &self.path {
            let parent = path.parent().ok_or("无效保存目录")?;
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let lock = path.with_extension("lock");
            let _guard = Lock::acquire(lock)?;
            if read(path)? != self.expected {
                return Err("本地文档已被其他实例修改；请重开应用后编辑".into());
            }
            let bytes = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > LIMIT {
                return Err("本地文档超过 4 MiB".into());
            }
            let tmp = path.with_extension("pending");
            let mut f = OpenOptions::new()
                .write(true)
                .mode(0o600)
                .create_new(true)
                .open(&tmp)
                .map_err(|_| "存在未完成保存文件；请保留并检查 .pending 文件")?;
            let result = (|| {
                f.write_all(&bytes)?;
                f.sync_all()?;
                fs::rename(&tmp, path)
            })()
            .map_err(|e: std::io::Error| e.to_string());
            if result.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            result?;
            self.expected = Some(bytes);
        }
        self.draft = next;
        Ok(())
    }
}
struct Lock(PathBuf);
impl Lock {
    fn acquire(path: PathBuf) -> Result<Self, String> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "文档正在保存或上次保存中断；请检查 .lock 文件")?;
        Ok(Self(path))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
