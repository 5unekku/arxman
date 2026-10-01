use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use anyhow::{bail, Context, Result};
use walkdir::WalkDir;

use crate::format::Format;

pub fn run(inputs: &[PathBuf], output: &Path, format_override: Option<&str>, level: Option<u32>) -> Result<()> {
    if inputs.is_empty() {
        bail!("no input files specified");
    }

    let fmt = if let Some(f) = format_override {
        Format::from_name(f).ok_or_else(|| anyhow::anyhow!("unknown format: {}", f))?
    } else {
        Format::from_extension(output).ok_or_else(|| {
            anyhow::anyhow!(
                "cannot infer format from '{}'; use -f to specify",
                output.display()
            )
        })?
    };

    if !fmt.can_compress() {
        if let Some(suggestion) = fmt.tar_equivalent() {
            bail!(
                "{} is a compression codec, not an archive format; use .{} instead",
                output.display(),
                suggestion
            );
        }
        bail!("compression to {} is not supported", output.display());
    }

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    println!("compressing -> {} ...", output.display());

    match fmt {
        Format::Zip | Format::Jar => compress_zip(inputs, output, level),
        Format::Tar => compress_tar(inputs, output),
        Format::TarGz => compress_tar_gz(inputs, output, level),
        Format::TarBz2 => compress_tar_bz2(inputs, output, level),
        Format::TarXz => compress_tar_xz(inputs, output, level),
        Format::TarZst => compress_tar_zst(inputs, output, level),
        Format::SevenZip => compress_7z(inputs, output),
        _ => unreachable!(),
    }
}

// --- file gathering ---

fn gather(inputs: &[PathBuf], output: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    let out_canon = output.canonicalize().ok();
    let mut pairs = Vec::new();
    for input in inputs {
        let canonical = input.canonicalize()
            .with_context(|| format!("accessing {}", input.display()))?;
        let parent = canonical.parent().unwrap_or(Path::new("/"));
        for entry in WalkDir::new(&canonical).sort_by_file_name() {
            let entry = entry?;
            let arc = entry.path().strip_prefix(parent)?.to_path_buf();
            if arc.as_os_str().is_empty() { continue; }
            if Some(entry.path()) == out_canon.as_deref() { continue; }
            pairs.push((entry.path().to_path_buf(), arc));
        }
    }
    Ok(pairs)
}

// --- zip ---

fn compress_zip(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let file = File::create(output)?;
    let mut zip = zip::ZipWriter::new(file);
    let base = zip::write::SimpleFileOptions::default()
        .compression_level(level.map(|l| l as i64));

    for (src, arc) in gather(inputs, output)? {
        let arc_str = arc.to_string_lossy().replace('\\', "/");
        let options = match fs::metadata(&src) {
            Ok(m) => base.unix_permissions(unix_mode(&m)),
            Err(_) => base,
        };
        if src.is_dir() {
            zip.add_directory(arc_str, options)?;
        } else {
            zip.start_file(arc_str, options)?;
            io::copy(&mut File::open(&src)?, &mut zip)?;
        }
    }
    zip.finish()?;
    Ok(())
}

// --- tar ---

fn compress_tar(inputs: &[PathBuf], output: &Path) -> Result<()> {
    let mut builder = tar::Builder::new(File::create(output)?);
    append_all(&mut builder, inputs, output)?;
    builder.finish()?;
    Ok(())
}

fn append_all<W: Write>(builder: &mut tar::Builder<W>, inputs: &[PathBuf], output: &Path) -> Result<()> {
    for (src, arc) in gather(inputs, output)? {
        if src.is_dir() {
            builder.append_dir(&arc, &src)?;
        } else {
            builder.append_path_with_name(&src, &arc)?;
        }
    }
    Ok(())
}

// --- tar.gz ---

fn compress_tar_gz(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 0, 9, 6);
    let enc = flate2::write::GzEncoder::new(File::create(output)?, flate2::Compression::new(lv));
    let mut builder = tar::Builder::new(enc);
    append_all(&mut builder, inputs, output)?;
    builder.finish()?;
    builder.into_inner()?.finish()?;
    Ok(())
}

// --- tar.bz2 ---

fn compress_tar_bz2(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 1, 9, 6);
    let enc = bzip2::write::BzEncoder::new(File::create(output)?, bzip2::Compression::new(lv));
    let mut builder = tar::Builder::new(enc);
    append_all(&mut builder, inputs, output)?;
    builder.finish()?;
    builder.into_inner()?.finish()?;
    Ok(())
}

// --- tar.xz ---

fn compress_tar_xz(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 0, 9, 6);
    let enc = xz2::write::XzEncoder::new(File::create(output)?, lv);
    let mut builder = tar::Builder::new(enc);
    append_all(&mut builder, inputs, output)?;
    builder.finish()?;
    builder.into_inner()?.finish()?;
    Ok(())
}

// --- tar.zst ---

fn compress_tar_zst(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 1, 22, 3) as i32;
    let enc = zstd::Encoder::new(File::create(output)?, lv)?;
    let mut builder = tar::Builder::new(enc);
    append_all(&mut builder, inputs, output)?;
    builder.finish()?;
    builder.into_inner()?.finish()?;
    Ok(())
}

// --- 7z ---

fn compress_7z(inputs: &[PathBuf], output: &Path) -> Result<()> {
    let mut writer = sevenz_rust::SevenZWriter::create(output)
        .with_context(|| format!("creating {}", output.display()))?;
    for (src, arc) in gather(inputs, output)? {
        let name = arc.to_string_lossy().replace('\\', "/");
        let entry = sevenz_rust::SevenZArchiveEntry::from_path(&src, name);
        if src.is_dir() {
            writer.push_archive_entry::<&[u8]>(entry, None)?;
        } else {
            writer.push_archive_entry(entry, Some(File::open(&src)?))?;
        }
    }
    writer.finish()?;
    Ok(())
}

fn clamp(level: Option<u32>, min: u32, max: u32, default: u32) -> u32 {
    level.unwrap_or(default).clamp(min, max)
}

#[cfg(unix)]
fn unix_mode(m: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    m.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn unix_mode(m: &fs::Metadata) -> u32 {
    if m.is_dir() { 0o755 } else { 0o644 }
}
