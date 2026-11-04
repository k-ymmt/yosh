//! Delivering an expanded here-document body as a readable fd.

use std::os::fd::RawFd;
use std::os::unix::io::IntoRawFd;

/// Heredoc bodies at or below this size are delivered via an anonymous
/// pipe, written in full before any reader is attached. POSIX only
/// guarantees a pipe absorbs PIPE_BUF (>= 512) bytes without a reader;
/// real capacities are 16-64 KiB. 4096 stays conservatively below any
/// real capacity so the up-front write cannot block. Larger bodies are
/// spooled through an unlinked temp file instead.
const HEREDOC_PIPE_MAX: usize = 4096;

/// Produce a readable fd that yields `body`, without blocking the shell
/// regardless of body size.
pub(super) fn heredoc_source_fd(body: &[u8]) -> Result<RawFd, String> {
    use std::io::{Seek, SeekFrom, Write};
    use std::os::unix::io::FromRawFd;

    if body.len() <= HEREDOC_PIPE_MAX {
        let mut fds: [RawFd; 2] = [0; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } == -1 {
            return Err(format!("pipe: {}", std::io::Error::last_os_error()));
        }
        let (read_fd, write_fd) = (fds[0], fds[1]);
        {
            let mut write_file = unsafe { std::fs::File::from_raw_fd(write_fd) };
            let _ = write_file.write_all(body);
            // drop closes write_fd
        }
        return Ok(read_fd);
    }

    // Spool via an unlinked temp file (what real shells do): the fd
    // stays valid, the name disappears immediately.
    let template = std::env::temp_dir().join("yosh-heredoc.XXXXXX");
    let mut template_bytes = {
        use std::os::unix::ffi::OsStrExt;
        template.as_os_str().as_bytes().to_vec()
    };
    template_bytes.push(0);
    let fd = unsafe { libc::mkstemp(template_bytes.as_mut_ptr() as *mut libc::c_char) };
    if fd == -1 {
        return Err(format!(
            "heredoc: mkstemp: {}",
            std::io::Error::last_os_error()
        ));
    }
    unsafe { libc::unlink(template_bytes.as_ptr() as *const libc::c_char) };

    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    file.write_all(body)
        .map_err(|e| format!("heredoc: write: {}", e))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("heredoc: seek: {}", e))?;
    Ok(file.into_raw_fd())
}
