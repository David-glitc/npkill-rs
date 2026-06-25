use std::path::Path;

/// A fast directory entry with just the fields we need.
#[derive(Debug)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

/// Read directory entries using raw `getdents64` syscall on Linux.
/// Falls back to std::fs::read_dir on other platforms.
#[cfg(target_os = "linux")]
pub fn read_dir_fast(path: &Path) -> std::io::Result<Vec<DirEntry>> {
    let file = std::fs::File::open(path)?;
    let fd = {
        use std::os::unix::io::AsRawFd;
        file.as_raw_fd()
    };

    let mut buf = [0u8; 8192];
    let mut entries = Vec::with_capacity(64);

    loop {
        let n = unsafe {
            libc::syscall(
                libc::SYS_getdents64,
                fd,
                buf.as_mut_ptr(),
                buf.len(),
            )
        };

        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }

        if n == 0 {
            break;
        }

        let n = n as usize;
        let mut offset = 0usize;

        while offset < n {
            let dirent = unsafe { buf.as_ptr().add(offset) };
            // linux_dirent64:
            //   ino64_t        d_ino       (8 bytes)
            //   off64_t        d_off       (8 bytes)
            //   unsigned short d_reclen    (2 bytes)
            //   unsigned char  d_type      (1 byte)
            //   char           d_name[]    (null-terminated)
            let d_reclen = unsafe { *(dirent.add(16) as *const u16) };
            let d_type = unsafe { *dirent.add(18) };
            let d_name_ptr = unsafe { dirent.add(19) };

            let name_len = d_reclen as usize - 19;
            let name_bytes = unsafe { std::slice::from_raw_parts(d_name_ptr, name_len) };
            let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_len);
            let name = String::from_utf8_lossy(&name_bytes[..name_end]).to_string();

            if name != "." && name != ".." {
                let is_dir = d_type == libc::DT_DIR;
                entries.push(DirEntry { name, is_dir });
            }

            offset += d_reclen as usize;
        }
    }

    Ok(entries)
}

/// Fallback for non-Linux or when getdents64 isn't available.
#[cfg(not(target_os = "linux"))]
pub fn read_dir_fast(path: &Path) -> std::io::Result<Vec<DirEntry>> {
    let mut entries = Vec::with_capacity(64);
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        entries.push(DirEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            is_dir: ft.is_dir(),
        });
    }
    Ok(entries)
}
