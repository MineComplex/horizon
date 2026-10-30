//! Helpers the java.io/java.nio ports share: Java's `byte[]` (`Vec<i8>`) seen as Rust bytes, and the
//! exceptions the library throws (all `crate::java::lang::Throwable`, told apart by class name).
use crate::java::lang::Throwable;

/// `byte[]` as `&[u8]` (same layout).
pub fn as_u8(b: &[i8]) -> &[u8] {
    // SAFETY: i8 and u8 have the same size, alignment and validity.
    unsafe { std::slice::from_raw_parts(b.as_ptr() as *const u8, b.len()) }
}

/// `byte[]` as `&mut [u8]` (same layout).
pub fn as_u8_mut(b: &mut [i8]) -> &mut [u8] {
    // SAFETY: as in `as_u8`.
    unsafe { std::slice::from_raw_parts_mut(b.as_mut_ptr() as *mut u8, b.len()) }
}

/// Rust bytes as `byte[]`.
pub fn as_i8(b: &[u8]) -> &[i8] {
    // SAFETY: as in `as_u8`.
    unsafe { std::slice::from_raw_parts(b.as_ptr() as *const i8, b.len()) }
}

/// Rust bytes as `&mut byte[]`.
pub fn as_i8_mut(b: &mut [u8]) -> &mut [i8] {
    // SAFETY: as in `as_u8`.
    unsafe { std::slice::from_raw_parts_mut(b.as_mut_ptr() as *mut i8, b.len()) }
}

/// `Vec<u8>` into Java's `byte[]` without copying.
pub fn to_i8_vec(v: Vec<u8>) -> Vec<i8> {
    let mut v = std::mem::ManuallyDrop::new(v);
    // SAFETY: same layout; the allocation is handed over unchanged.
    unsafe { Vec::from_raw_parts(v.as_mut_ptr() as *mut i8, v.len(), v.capacity()) }
}

/// Java's `byte[]` into `Vec<u8>` without copying.
pub fn to_u8_vec(v: Vec<i8>) -> Vec<u8> {
    let mut v = std::mem::ManuallyDrop::new(v);
    // SAFETY: same layout; the allocation is handed over unchanged.
    unsafe { Vec::from_raw_parts(v.as_mut_ptr() as *mut u8, v.len(), v.capacity()) }
}

/// A Java exception of class `class` (fully qualified, `java.io.IOException`) with `message`.
/// Every exception the library ports throw is built here.
pub fn throwable(class: &str, message: impl Into<String>) -> Throwable {
    Throwable::new(class, message.into())
}

/// `java.io.IOException`.
pub fn io_exception(message: impl Into<String>) -> Throwable {
    throwable("java.io.IOException", message)
}

/// `java.io.EOFException`.
pub fn eof_exception() -> Throwable {
    throwable("java.io.EOFException", "")
}

/// `java.lang.IndexOutOfBoundsException` (unchecked: the ports panic with it, as Java throws).
pub fn index_out_of_bounds(off: i32, len: i32, size: usize) -> ! {
    panic!("java.lang.IndexOutOfBoundsException: Range [{off}, {off} + {len}) out of bounds for length {size}")
}

/// Checks Java's `(off, len)` range on an array of `size`.
pub fn check_range(off: i32, len: i32, size: usize) -> (usize, usize) {
    if off < 0 || len < 0 || off as i64 + len as i64 > size as i64 {
        index_out_of_bounds(off, len, size);
    }
    (off as usize, off as usize + len as usize)
}

/// A `std::io::Error` as the Java exception `java.io` would throw for it.
pub fn from_io(e: std::io::Error) -> Throwable {
    use std::io::ErrorKind::*;
    match e.kind() {
        UnexpectedEof => throwable("java.io.EOFException", e.to_string()),
        NotFound => throwable("java.io.FileNotFoundException", e.to_string()),
        InvalidData => throwable("java.io.IOException", e.to_string()),
        _ => Throwable::from(e),
    }
}

/// A `std::io::Error` met on `path` as the `java.nio.file` exception `Files` would throw
/// (`NoSuchFileException`, `FileAlreadyExistsException`, `AccessDeniedException`, ...).
pub fn from_io_path(e: std::io::Error, path: &std::path::Path) -> Throwable {
    use std::io::ErrorKind::*;
    let p = path.display().to_string();
    match e.kind() {
        NotFound => throwable("java.nio.file.NoSuchFileException", p),
        AlreadyExists => throwable("java.nio.file.FileAlreadyExistsException", p),
        PermissionDenied => throwable("java.nio.file.AccessDeniedException", p),
        DirectoryNotEmpty => throwable("java.nio.file.DirectoryNotEmptyException", p),
        NotADirectory => throwable("java.nio.file.NotDirectoryException", p),
        _ => throwable("java.nio.file.FileSystemException", format!("{p}: {e}")),
    }
}

/// A `Throwable` back into a `std::io::Error` (for `std::io::Read`/`Write` impls over Java streams).
pub fn to_io(t: Throwable) -> std::io::Error {
    std::io::Error::other(t.to_string())
}
