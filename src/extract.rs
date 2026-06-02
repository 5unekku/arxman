use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use anyhow::{bail, Context, Result};

use crate::detect;
use crate::format::{archive_stem, Format};

pub fn run(files: &[PathBuf], format_override: Option<&str>) -> Result<()> {
    if files.is_empty() {
        bail!("no files specified");
    }

    let (archives, dest) = split_args(files, format_override)?;
    if archives.is_empty() {
        bail!("no archives specified");
    }

    fs::create_dir_all(&dest).with_context(|| format!("creating {}", dest.display()))?;

    for archive in &archives {
        extract_archive(archive, &dest, format_override)?;
    }
    Ok(())
}

/// split positional args into (archives, destination)
fn split_args(files: &[PathBuf], format_override: Option<&str>) -> Result<(Vec<PathBuf>, PathBuf)> {
    let last = files.last().unwrap();
    let last_is_dest = if last.is_dir() {
        true
    } else if !last.exists() {
        // non-existent path is dest only when there are other args before it
        files.len() > 1
    } else {
        // existing file: dest if no archive format can be detected
        format_override.is_none()
            && Format::from_extension(last).is_none()
            && detect::by_magic(last).ok().flatten().is_none()
    };

    Ok(if last_is_dest && files.len() > 1 {
        (files[..files.len() - 1].to_vec(), last.clone())
    } else {
        (files.to_vec(), PathBuf::from("."))
    })
}

pub fn extract_archive(path: &Path, dest: &Path, format_override: Option<&str>) -> Result<()> {
    let fmt = if let Some(f) = format_override {
        Format::from_name(f).ok_or_else(|| anyhow::anyhow!("unknown format: {}", f))?
    } else {
        detect::detect(path)
            .with_context(|| format!("reading {}", path.display()))?
            .ok_or_else(|| anyhow::anyhow!("cannot detect format of {}", path.display()))?
    };

    println!("extracting {} ...", path.display());
    match fmt {
        Format::Zip | Format::Jar => extract_zip(path, dest),
        Format::Tar => smart_extract(|d| unpack_tar(path, d), path, dest),
        Format::TarGz => smart_extract(|d| unpack_tar_gz(path, d), path, dest),
        Format::TarBz2 => smart_extract(|d| unpack_tar_bz2(path, d), path, dest),
        Format::TarXz => smart_extract(|d| unpack_tar_xz(path, d), path, dest),
        Format::TarZst => smart_extract(|d| unpack_tar_zst(path, d), path, dest),
        Format::Gz => extract_gz(path, dest),
        Format::Bz2 => extract_bz2(path, dest),
        Format::Xz => extract_xz(path, dest),
        Format::Zst => extract_zst(path, dest),
        Format::Zlib => extract_zlib(path, dest),
        Format::SevenZip => extract_7z(path, dest),
        Format::Rar => extract_rar(path, dest),
    }
}

// --- smart wrapper logic ---

/// extract to a temp dir, then move to dest with optional wrapper dir
fn smart_extract(raw: impl FnOnce(&Path) -> Result<()>, archive: &Path, dest: &Path) -> Result<()> {
    let tmp = dest.join(format!(
        ".arx-tmp-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&tmp)?;

    if let Err(e) = raw(&tmp) {
        fs::remove_dir_all(&tmp).ok();
        return Err(e);
    }

    let top: Vec<_> = fs::read_dir(&tmp)?
        .filter_map(|e| e.ok())
        .collect();

    if top.is_empty() {
        fs::remove_dir_all(&tmp).ok();
        return Ok(());
    }

    if top.len() == 1 {
        // single top-level item: move directly to dest
        let src = top[0].path();
        let final_path = dest.join(top[0].file_name());
        move_into(src, final_path)?;
        fs::remove_dir_all(&tmp).ok();
    } else {
        // multiple items: wrap in stem-named directory
        let wrapper = dest.join(archive_stem(archive));
        if wrapper.exists() {
            fs::remove_dir_all(&wrapper)?;
        }
        fs::rename(&tmp, &wrapper)?;
        println!("  -> {}/", wrapper.display());
    }
    Ok(())
}

fn move_into(src: PathBuf, dest: PathBuf) -> Result<()> {
    if dest.exists() {
        if dest.is_dir() { fs::remove_dir_all(&dest)?; } else { fs::remove_file(&dest)?; }
    }
    fs::rename(&src, &dest)?;
    println!("  -> {}", dest.display());
    Ok(())
}

// --- zip / jar ---

fn extract_zip(path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    // scan top-level names to decide if a wrapper dir is needed
    let mut tops = HashSet::new();
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i)?;
        let top = entry.name().split('/').next().unwrap_or("").to_string();
        if !top.is_empty() { tops.insert(top); }
    }

    let extract_dest = if tops.len() > 1 {
        let d = dest.join(archive_stem(path));
        fs::create_dir_all(&d)?;
        d
    } else {
        dest.to_path_buf()
    };

    // re-open for extraction (ZipArchive consumed the scan)
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    archive.extract(&extract_dest)?;
    println!("  -> {}", extract_dest.display());
    Ok(())
}

// --- tar variants ---

fn unpack_tar(path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(path)?;
    tar::Archive::new(file).unpack(dest)?;
    Ok(())
}

fn unpack_tar_gz(path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    tar::Archive::new(decoder).unpack(dest)?;
    Ok(())
}

fn unpack_tar_bz2(path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(path)?;
    let decoder = bzip2::read::BzDecoder::new(file);
    tar::Archive::new(decoder).unpack(dest)?;
    Ok(())
}

fn unpack_tar_xz(path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(path)?;
    let decoder = xz2::read::XzDecoder::new(file);
    tar::Archive::new(decoder).unpack(dest)?;
    Ok(())
}

fn unpack_tar_zst(path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(path)?;
    let decoder = zstd::Decoder::new(file)?;
    tar::Archive::new(decoder).unpack(dest)?;
    Ok(())
}

// --- single-stream compressed formats ---
// for these, peek inside to detect a hidden tar layer (e.g. file with no extension)

fn extract_gz(path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(path)?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut peek = [0u8; 512];
    let n = decoder.read(&mut peek).unwrap_or(0);
    if detect::is_tar_bytes(&peek[..n]) {
        return smart_extract(|d| unpack_tar_gz(path, d), path, dest);
    }
    let out = single_stream_out(path, dest);
    let file = File::open(path)?;
    let mut dec = flate2::read::GzDecoder::new(file);
    io::copy(&mut dec, &mut File::create(&out)?)?;
    println!("  -> {}", out.display());
    Ok(())
}

fn extract_bz2(path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(path)?;
    let mut decoder = bzip2::read::BzDecoder::new(file);
    let mut peek = [0u8; 512];
    let n = decoder.read(&mut peek).unwrap_or(0);
    if detect::is_tar_bytes(&peek[..n]) {
        return smart_extract(|d| unpack_tar_bz2(path, d), path, dest);
    }
    let out = single_stream_out(path, dest);
    let file = File::open(path)?;
    let mut dec = bzip2::read::BzDecoder::new(file);
    io::copy(&mut dec, &mut File::create(&out)?)?;
    println!("  -> {}", out.display());
    Ok(())
}

fn extract_xz(path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(path)?;
    let mut decoder = xz2::read::XzDecoder::new(file);
    let mut peek = [0u8; 512];
    let n = decoder.read(&mut peek).unwrap_or(0);
    if detect::is_tar_bytes(&peek[..n]) {
        return smart_extract(|d| unpack_tar_xz(path, d), path, dest);
    }
    let out = single_stream_out(path, dest);
    let file = File::open(path)?;
    let mut dec = xz2::read::XzDecoder::new(file);
    io::copy(&mut dec, &mut File::create(&out)?)?;
    println!("  -> {}", out.display());
    Ok(())
}

fn extract_zst(path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(path)?;
    let mut decoder = zstd::Decoder::new(file)?;
    let mut peek = [0u8; 512];
    let n = decoder.read(&mut peek).unwrap_or(0);
    if detect::is_tar_bytes(&peek[..n]) {
        return smart_extract(|d| unpack_tar_zst(path, d), path, dest);
    }
    let out = single_stream_out(path, dest);
    let file = File::open(path)?;
    let mut dec = zstd::Decoder::new(file)?;
    io::copy(&mut dec, &mut File::create(&out)?)?;
    println!("  -> {}", out.display());
    Ok(())
}

fn extract_zlib(path: &Path, dest: &Path) -> Result<()> {
    let out = single_stream_out(path, dest);
    let file = File::open(path)?;
    let mut dec = flate2::read::ZlibDecoder::new(file);
    io::copy(&mut dec, &mut File::create(&out)?)?;
    println!("  -> {}", out.display());
    Ok(())
}

/// determine output path for single-stream decompression (strip compression ext)
fn single_stream_out(path: &Path, dest: &Path) -> PathBuf {
    let name = if path.extension().is_some() {
        path.file_stem()
            .unwrap_or(path.file_name().unwrap())
            .to_string_lossy()
            .to_string()
    } else {
        format!("{}.out", path.file_name().unwrap().to_string_lossy())
    };
    dest.join(name)
}

// --- 7z ---

fn extract_7z(path: &Path, dest: &Path) -> Result<()> {
    sevenz_rust::decompress_file(path, dest)
        .with_context(|| format!("7z extraction failed for {}", path.display()))?;
    println!("  -> {}", dest.display());
    Ok(())
}

// --- rar (system command fallback) ---

fn extract_rar(path: &Path, dest: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    let dest_str = dest.to_string_lossy();

    // try unrar first, then 7z
    if which("unrar") {
        let status = std::process::Command::new("unrar")
            .args(["x", "-y", &path_str, &format!("{}/", dest_str)])
            .status()?;
        if status.success() {
            return Ok(());
        }
    }
    if which("7z") {
        let status = std::process::Command::new("7z")
            .args(["x", &path_str, &format!("-o{}", dest_str), "-y"])
            .status()?;
        if status.success() {
            return Ok(());
        }
    }
    bail!("rar extraction requires unrar or 7z to be installed")
}

fn which(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
