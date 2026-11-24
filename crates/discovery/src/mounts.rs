//! The mount table, as `/proc/self/mountinfo` gives it.

use std::path::PathBuf;

/// One line of `/proc/self/mountinfo`: where something is mounted, and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub point: PathBuf,
    pub fstype: String,
}

/// Every mount in a mountinfo table, in the table's order.
pub fn parse_mountinfo(table: &str) -> Vec<Mount> {
    table
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let point = left.split(' ').nth(4)?;
            let fstype = right.split(' ').next()?;
            Some(Mount {
                point: PathBuf::from(unescape_mountinfo(point)),
                fstype: fstype.to_string(),
            })
        })
        .collect()
}

/// This process's mounts, read where the kernel keeps them rather than from
/// `mount`, whose output is for people.
pub fn mounts_here() -> Vec<Mount> {
    std::fs::read_to_string("/proc/self/mountinfo")
        .map(|t| parse_mountinfo(&t))
        .unwrap_or_default()
}

/// mountinfo spells a space, tab, newline and backslash as octal escapes.
pub fn unescape_mountinfo(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 4 <= bytes.len()
            && let Ok(code) = u8::from_str_radix(&field[i + 1..i + 4], 8)
        {
            out.push(code);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
