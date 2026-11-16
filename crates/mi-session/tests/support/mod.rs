pub type Result<T> = std::result::Result<T, &'static str>;

pub fn temp_name(name: &str) -> Result<()> {
    if name.len() == 19
        && name.starts_with("mi_")
        && name.ends_with(".txt")
        && name.as_bytes()[3..15]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        Ok(())
    } else {
        Err("invalid_temp_target")
    }
}

pub mod hardware;
