//! Host facts behind the terminal memory limits (HS2-Y5DYC0). The limit settings themselves and
//! their memory-derived defaults live in `hotsheet_ticketing::terminal_limits`; this crate only
//! reports the machine's physical memory and keeps a spawned replay buffer within bounds.

/// The replay-buffer bounds a terminal accepts at spawn, in bytes (64 KiB to 64 MiB). Kept equal
/// to `hotsheet_ticketing::terminal_limits::REPLAY_BYTES_RANGE` (a server test checks it).
pub const REPLAY_BYTES_BOUNDS: (usize, usize) = (64 * 1024, 64 * 1024 * 1024);

/// The machine's physical memory in bytes, when the platform reports it.
pub fn physical_memory_bytes() -> Option<u64> {
    #[cfg(unix)]
    {
        // SAFETY: `sysconf` only reads system configuration values.
        let (pages, page_size) = unsafe {
            (
                libc::sysconf(libc::_SC_PHYS_PAGES),
                libc::sysconf(libc::_SC_PAGESIZE),
            )
        };
        if pages > 0 && page_size > 0 {
            return u64::try_from(pages)
                .ok()?
                .checked_mul(u64::try_from(page_size).ok()?);
        }
        None
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_reports_its_physical_memory() {
        #[cfg(unix)]
        assert!(physical_memory_bytes().is_some_and(|bytes| bytes > 256 * 1024 * 1024));
    }
}
