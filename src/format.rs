use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum Format {
    Zip,
    Jar,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
    Gz,
    Bz2,
    Xz,
    Zst,
    SevenZip,
    Rar,
}

impl Format {
    pub fn from_extension(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?.to_lowercase();
        // compound extensions checked first (order matters)
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") { return Some(Self::TarGz); }
        if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") || name.ends_with(".tbz") { return Some(Self::TarBz2); }
        if name.ends_with(".tar.xz") || name.ends_with(".txz") { return Some(Self::TarXz); }
        if name.ends_with(".tar.zst") || name.ends_with(".tar.zstd") || name.ends_with(".tzst") { return Some(Self::TarZst); }
        let ext = path.extension()?.to_str()?.to_lowercase();
        match ext.as_str() {
            "zip" => Some(Self::Zip),
            "jar" => Some(Self::Jar),
            "tar" => Some(Self::Tar),
            "gz" | "gzip" => Some(Self::Gz),
            "bz2" | "bzip2" => Some(Self::Bz2),
            "xz" => Some(Self::Xz),
            "zst" | "zstd" => Some(Self::Zst),
            "7z" => Some(Self::SevenZip),
            "rar" => Some(Self::Rar),
            _ => None,
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_lowercase().trim_start_matches('.') {
            "zip" => Some(Self::Zip),
            "jar" => Some(Self::Jar),
            "tar" => Some(Self::Tar),
            "tar.gz" | "tgz" => Some(Self::TarGz),
            "tar.bz2" | "tbz2" | "tbz" => Some(Self::TarBz2),
            "tar.xz" | "txz" => Some(Self::TarXz),
            "tar.zst" | "tar.zstd" | "tzst" => Some(Self::TarZst),
            "gz" | "gzip" => Some(Self::Gz),
            "bz2" | "bzip2" => Some(Self::Bz2),
            "xz" => Some(Self::Xz),
            "zst" | "zstd" => Some(Self::Zst),
            "7z" | "sevenz" => Some(Self::SevenZip),
            "rar" => Some(Self::Rar),
            _ => None,
        }
    }

    /// true only for formats that are actual archives (not bare compression streams)
    pub fn can_compress(&self) -> bool {
        matches!(self, Self::Zip | Self::Jar | Self::Tar | Self::TarGz | Self::TarBz2 | Self::TarXz | Self::TarZst | Self::SevenZip)
    }

    /// human-readable suggestion for bare codec formats
    pub fn tar_equivalent(&self) -> Option<&'static str> {
        match self {
            Self::Gz => Some("tar.gz"),
            Self::Bz2 => Some("tar.bz2"),
            Self::Xz => Some("tar.xz"),
            Self::Zst => Some("tar.zst"),
            _ => None,
        }
    }
}

/// strip archive extensions to get a clean stem for wrapper dir naming
pub fn archive_stem(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let compound = [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.zstd",
        ".tgz", ".tbz2", ".tbz", ".txz", ".tzst",
    ];
    for suffix in &compound {
        if let Some(base) = name.strip_suffix(suffix) {
            return base.to_string();
        }
    }
    if let Some(pos) = name.rfind('.') {
        name[..pos].to_string()
    } else {
        name.to_string()
    }
}
