use std::path::Path;
use walkdir::DirEntry;

#[inline]
pub fn strip_extension(source: &str) -> &str {
    if let Some(ext_index) = source.find('.') {
        &source[..ext_index]
    } else {
        source
    }
}

#[inline]
pub fn has_extension<P: AsRef<Path>>(path: P, ext: &str) -> bool {
    path.as_ref()
        .extension()
        .map(|e| e.to_str().unwrap_or("") == ext)
        .unwrap_or(false)
}

#[inline]
pub fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
}
