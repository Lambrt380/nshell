use std::env;
use std::fs;
use std::path::PathBuf;

pub fn directory() -> PathBuf {
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        PathBuf::from(path).join("nshell")
    } else {
        PathBuf::from(env::var_os("HOME").unwrap_or_else(|| ".".into())).join(".local/state/nshell")
    }
}

pub fn ensure_directory() -> std::io::Result<PathBuf> {
    let path = directory();
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn hash(bytes: &[u8]) -> String {
    // FNV-1a is used only as a change detector, not for security.
    let mut value = 0xcbf29ce484222325_u64;
    for byte in bytes {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100000001b3);
    }
    format!("{value:016x}")
}
