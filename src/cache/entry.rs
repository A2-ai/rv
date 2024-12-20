use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};

#[derive(Debug, PartialEq, Clone)]
enum CacheEntry {
    // TODO: pass a timestamp to database (mtime or ctime?)
    Database,
    Tarball,
}

/// Just a basic base64 without padding
fn encode_repository_url(url: &str) -> String {
    STANDARD_NO_PAD.encode(url)
}