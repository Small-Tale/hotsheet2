/* HS2-ZW0PVE: MSVC 14.52 rejects `__has_include_next(...)` in an #if even after a false
 * `defined(__clang__) &&`, which breaks the compat stdatomic.h of the dav1d 1.4.3 that
 * libdav1d-sys 0.7.1 vendors (C7780). Upstream dav1d a34e0685 adds this same fallback.
 * The Windows x64 Platforms jobs force-include it into C compiles through CFLAGS. */
#ifndef __has_include_next
#define __has_include_next(x) 0
#endif
