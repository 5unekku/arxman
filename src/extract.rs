use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use anyhow::{bail, Context, Result};

use crate::detect;
use crate::format::{archive_stem, Format};

#[derive(Clone, Copy)]
pub enum WrapperMode {
    Sub,    // default: wrap in a subfolder named after the archive (extensions stripped)
    Bare,   // -b: extract contents directly into dest
}

pub fn run(files: &[PathBuf], format_override: Option<&str>, mode: WrapperMode) -> Result<()> {
    if files.is_empty() {
        bail!("no files specified");
    }

    let (archives, dest) = split_args(files, format_override)?;
    if archives.is_empty() {
        bail!("no archives specified");
    }

    fs::create_dir_all(&dest).with_context(|| format!("creating {}", dest.display()))?;

    for archive in &archives {
        extract_archive(archive, &dest, format_override, mode)?;
    }
    Ok(())
}

fn split_args(files: &[PathBuf], format_override: Option<&str>) -> Result<(Vec<PathBuf>, PathBuf)> {
    let last = files.last().unwrap();
    let last_is_dest = if last.is_dir() {
        true
    } else if !last.exists() {
        files.len() > 1
    } else {
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

pub fn extract_archive(path: &Path, dest: &Path, format_override: Option<&str>, mode: WrapperMode) -> Result<()> {
    let fmt = if let Some(f) = format_override {
        Format::from_name(f).ok_or_else(|| anyhow::anyhow!("unknown format: {}", f))?
    } else {
        detect::detect(path)
            .with_context(|| format!("reading {}", path.display()))?
            .ok_or_else(|| anyhow::anyhow!("cannot detect format of {}", path.display()))?
    };

    println!("extracting {} ...", path.display());
    match fmt {
        Format::Zip | Format::Jar => extract_zip(path, dest, mode),
        Format::Tar    => smart_extract(|d| unpack_tar(path, d),    path, dest, mode),
        Format::TarGz  => smart_extract(|d| unpack_tar_gz(path, d), path, dest, mode),
        Format::TarBz2 => smart_extract(|d| unpack_tar_bz2(path, d), path, dest, mode),
        Format::TarXz  => smart_extract(|d| unpack_tar_xz(path, d), path, dest, mode),
        Format::TarZst => smart_extract(|d| unpack_tar_zst(path, d), path, dest, mode),
        Format::Gz  => extract_gz(path, dest, mode),
        Format::Bz2 => extract_bz2(path, dest, mode),
        Format::Xz  => extract_xz(path, dest, mode),
        Format::Zst => extract_zst(path, dest, mode),
        Format::SevenZip => smart_extract(|d| extract_7z(path, d), path, dest, mode),
        Format::Rar     => smart_extract(|d| extract_rar(path, d), path, dest, mode),
    }
}

// --- wrapper logic ---

fn smart_extract(raw: impl FnOnce(&Path) -> Result<()>, archive: &Path, dest: &Path, mode: WrapperMode) -> Result<()> {
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

    let top: Vec<_> = fs::read_dir(&tmp)?.filter_map(|e| e.ok()).collect();

    if top.is_empty() {
        fs::remove_dir_all(&tmp).ok();
        return Ok(());
    }

    let use_wrapper = !matches!(mode, WrapperMode::Bare);

    if use_wrapper {
        let wrapper = dest.join(archive_stem(archive));
        if wrapper.exists() {
            // merge into the existing folder rather than deleting it
            for entry in top {
                move_into(entry.path(), wrapper.join(entry.file_name()))?;
            }
            fs::remove_dir_all(&tmp).ok();
            println!("  -> {}/", wrapper.display());
            return Ok(());
        }
        fs::rename(&tmp, &wrapper)?;
        println!("  -> {}/", wrapper.display());
    } else {
        // move each top-level item directly into dest
        for entry in top {
            move_into(entry.path(), dest.join(entry.file_name()))?;
        }
        fs::remove_dir_all(&tmp).ok();
    }
    Ok(())
}

fn move_into(src: PathBuf, dest: PathBuf) -> Result<()> {
    merge_move(&src, &dest)?;
    println!("  -> {}", dest.display());
    Ok(())
}

/// move src to dest; directories merge, files overwrite (never deletes unrelated content)
fn merge_move(src: &Path, dest: &Path) -> Result<()> {
    let src_is_dir = src.symlink_metadata()?.is_dir();
    match dest.symlink_metadata() {
        Ok(meta) if meta.is_dir() && src_is_dir => {
            for entry in fs::read_dir(src)? {
                let entry = entry?;
                merge_move(&entry.path(), &dest.join(entry.file_name()))?;
            }
            fs::remove_dir(src)?;
        }
        Ok(meta) => {
            if meta.is_dir() { fs::remove_dir_all(dest)?; } else { fs::remove_file(dest)?; }
            fs::rename(src, dest)?;
        }
        Err(_) => fs::rename(src, dest)?,
    }
    Ok(())
}

// --- zip / jar ---

fn extract_zip(path: &Path, dest: &Path, mode: WrapperMode) -> Result<()> {
    let use_wrapper = !matches!(mode, WrapperMode::Bare);

    let extract_dest = if use_wrapper {
        let d = dest.join(archive_stem(path));
        fs::create_dir_all(&d)?;
        d
    } else {
        dest.to_path_buf()
    };

    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    archive.extract(&extract_dest)?;
    println!("  -> {}", extract_dest.display());
    Ok(())
}

// --- tar variants ---

fn unpack_tar(path: &Path, dest: &Path) -> Result<()> {
    tar::Archive::new(File::open(path)?).unpack(dest)?;
    Ok(())
}

fn unpack_tar_gz(path: &Path, dest: &Path) -> Result<()> {
    tar::Archive::new(flate2::read::GzDecoder::new(File::open(path)?)).unpack(dest)?;
    Ok(())
}

fn unpack_tar_bz2(path: &Path, dest: &Path) -> Result<()> {
    tar::Archive::new(bzip2::read::BzDecoder::new(File::open(path)?)).unpack(dest)?;
    Ok(())
}

fn unpack_tar_xz(path: &Path, dest: &Path) -> Result<()> {
    tar::Archive::new(xz2::read::XzDecoder::new(File::open(path)?)).unpack(dest)?;
    Ok(())
}

fn unpack_tar_zst(path: &Path, dest: &Path) -> Result<()> {
    tar::Archive::new(zstd::Decoder::new(File::open(path)?)?).unpack(dest)?;
    Ok(())
}

// --- single-stream (peek for hidden tar layer) ---

fn extract_gz(path: &Path, dest: &Path, mode: WrapperMode) -> Result<()> {
    let mut peek = [0u8; 512];
    let n = flate2::read::GzDecoder::new(File::open(path)?).read(&mut peek).unwrap_or(0);
    if detect::is_tar_bytes(&peek[..n]) {
        return smart_extract(|d| unpack_tar_gz(path, d), path, dest, mode);
    }
    let out = single_stream_out(path, dest);
    io::copy(&mut flate2::read::GzDecoder::new(File::open(path)?), &mut File::create(&out)?)?;
    println!("  -> {}", out.display());
    Ok(())
}

fn extract_bz2(path: &Path, dest: &Path, mode: WrapperMode) -> Result<()> {
    let mut peek = [0u8; 512];
    let n = bzip2::read::BzDecoder::new(File::open(path)?).read(&mut peek).unwrap_or(0);
    if detect::is_tar_bytes(&peek[..n]) {
        return smart_extract(|d| unpack_tar_bz2(path, d), path, dest, mode);
    }
    let out = single_stream_out(path, dest);
    io::copy(&mut bzip2::read::BzDecoder::new(File::open(path)?), &mut File::create(&out)?)?;
    println!("  -> {}", out.display());
    Ok(())
}

fn extract_xz(path: &Path, dest: &Path, mode: WrapperMode) -> Result<()> {
    let mut peek = [0u8; 512];
    let n = xz2::read::XzDecoder::new(File::open(path)?).read(&mut peek).unwrap_or(0);
    if detect::is_tar_bytes(&peek[..n]) {
        return smart_extract(|d| unpack_tar_xz(path, d), path, dest, mode);
    }
    let out = single_stream_out(path, dest);
    io::copy(&mut xz2::read::XzDecoder::new(File::open(path)?), &mut File::create(&out)?)?;
    println!("  -> {}", out.display());
    Ok(())
}

fn extract_zst(path: &Path, dest: &Path, mode: WrapperMode) -> Result<()> {
    let mut peek = [0u8; 512];
    let n = zstd::Decoder::new(File::open(path)?)?.read(&mut peek).unwrap_or(0);
    if detect::is_tar_bytes(&peek[..n]) {
        return smart_extract(|d| unpack_tar_zst(path, d), path, dest, mode);
    }
    let out = single_stream_out(path, dest);
    io::copy(&mut zstd::Decoder::new(File::open(path)?)?, &mut File::create(&out)?)?;
    println!("  -> {}", out.display());
    Ok(())
}

fn single_stream_out(path: &Path, dest: &Path) -> PathBuf {
    let name = if path.extension().is_some() {
        path.file_stem().unwrap_or(path.file_name().unwrap()).to_string_lossy().to_string()
    } else {
        format!("{}.out", path.file_name().unwrap().to_string_lossy())
    };
    dest.join(name)
}

// --- 7z ---

fn extract_7z(path: &Path, dest: &Path) -> Result<()> {
    sevenz_rust::decompress_file(path, dest)
        .with_context(|| format!("7z extraction failed for {}", path.display()))?;
    Ok(())
}

// --- rar ---

fn extract_rar(path: &Path, dest: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    let dest_str = dest.to_string_lossy();
    if which("unrar") {
        let status = std::process::Command::new("unrar")
            .args(["x", "-y", &path_str, &format!("{}/", dest_str)])
            .status()?;
        if status.success() { return Ok(()); }
    }
    if which("7z") {
        let status = std::process::Command::new("7z")
            .args(["x", &path_str, &format!("-o{}", dest_str), "-y"])
            .status()?;
        if status.success() { return Ok(()); }
    }
    bail!("rar extraction requires unrar or 7z to be installed")
}

fn which(cmd: &str) -> bool {
    std::process::Command::new("which").arg(cmd).output()
        .map(|o| o.status.success()).unwrap_or(false)
}
