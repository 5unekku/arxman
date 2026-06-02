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
                "cannot infer format from output name '{}'; use -f to specify",
                output.display()
            )
        })?
    };

    if !fmt.can_compress() {
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
        Format::Gz => compress_gz(inputs, output, level),
        Format::Bz2 => compress_bz2(inputs, output, level),
        Format::Xz => compress_xz(inputs, output, level),
        Format::Zst => compress_zst(inputs, output, level),
        Format::Zlib => compress_zlib(inputs, output, level),
        Format::SevenZip => compress_7z(inputs, output),
        Format::Rar => unreachable!(),
    }
}

// --- file gathering ---

/// walk inputs and return (source_path, archive_path) pairs
fn gather(inputs: &[PathBuf]) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut pairs = Vec::new();
    for input in inputs {
        let canonical = input.canonicalize()
            .with_context(|| format!("accessing {}", input.display()))?;
        let parent = canonical.parent().unwrap_or(Path::new("/"));
        for entry in WalkDir::new(&canonical).sort_by_file_name() {
            let entry = entry?;
            let arc = entry.path().strip_prefix(parent)?.to_path_buf();
            if arc == PathBuf::from("") { continue; }
            pairs.push((entry.path().to_path_buf(), arc));
        }
    }
    Ok(pairs)
}

// --- zip ---

fn compress_zip(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let file = File::create(output)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_level(level.map(|l| l as i64));

    for (src, arc) in gather(inputs)? {
        let arc_str = arc.to_string_lossy();
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

// --- tar (no compression) ---

fn compress_tar(inputs: &[PathBuf], output: &Path) -> Result<()> {
    let file = File::create(output)?;
    let mut builder = tar::Builder::new(file);
    append_all(&mut builder, inputs)?;
    builder.finish()?;
    Ok(())
}

fn append_all<W: Write>(builder: &mut tar::Builder<W>, inputs: &[PathBuf]) -> Result<()> {
    for (src, arc) in gather(inputs)? {
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
    let file = File::create(output)?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::new(lv));
    let mut builder = tar::Builder::new(enc);
    append_all(&mut builder, inputs)?;
    builder.finish()?;
    builder.into_inner()?.finish()?;
    Ok(())
}

// --- tar.bz2 ---

fn compress_tar_bz2(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 1, 9, 6);
    let file = File::create(output)?;
    let enc = bzip2::write::BzEncoder::new(file, bzip2::Compression::new(lv));
    let mut builder = tar::Builder::new(enc);
    append_all(&mut builder, inputs)?;
    builder.finish()?;
    builder.into_inner()?.finish()?;
    Ok(())
}

// --- tar.xz ---

fn compress_tar_xz(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 0, 9, 6);
    let file = File::create(output)?;
    let enc = xz2::write::XzEncoder::new(file, lv);
    let mut builder = tar::Builder::new(enc);
    append_all(&mut builder, inputs)?;
    builder.finish()?;
    builder.into_inner()?.finish()?;
    Ok(())
}

// --- tar.zst ---

fn compress_tar_zst(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 1, 22, 3) as i32;
    let file = File::create(output)?;
    let enc = zstd::Encoder::new(file, lv)?;
    let mut builder = tar::Builder::new(enc);
    append_all(&mut builder, inputs)?;
    builder.finish()?;
    builder.into_inner()?.finish()?;
    Ok(())
}

// --- single-stream formats (only make sense for a single input file) ---

fn single_input(inputs: &[PathBuf]) -> Result<&Path> {
    if inputs.len() != 1 || inputs[0].is_dir() {
        bail!("single-stream formats (.gz, .bz2, .xz, .zst, .zlib) require exactly one file input");
    }
    Ok(&inputs[0])
}

fn compress_gz(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 0, 9, 6);
    let src = single_input(inputs)?;
    let mut enc = flate2::write::GzEncoder::new(File::create(output)?, flate2::Compression::new(lv));
    io::copy(&mut File::open(src)?, &mut enc)?;
    enc.finish()?;
    Ok(())
}

fn compress_bz2(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 1, 9, 6);
    let src = single_input(inputs)?;
    let mut enc = bzip2::write::BzEncoder::new(File::create(output)?, bzip2::Compression::new(lv));
    io::copy(&mut File::open(src)?, &mut enc)?;
    enc.finish()?;
    Ok(())
}

fn compress_xz(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 0, 9, 6);
    let src = single_input(inputs)?;
    let mut enc = xz2::write::XzEncoder::new(File::create(output)?, lv);
    io::copy(&mut File::open(src)?, &mut enc)?;
    enc.finish()?;
    Ok(())
}

fn compress_zst(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 1, 22, 3) as i32;
    let src = single_input(inputs)?;
    let mut enc = zstd::Encoder::new(File::create(output)?, lv)?;
    io::copy(&mut File::open(src)?, &mut enc)?;
    enc.finish()?;
    Ok(())
}

fn compress_zlib(inputs: &[PathBuf], output: &Path, level: Option<u32>) -> Result<()> {
    let lv = clamp(level, 0, 9, 6);
    let src = single_input(inputs)?;
    let mut enc = flate2::write::ZlibEncoder::new(File::create(output)?, flate2::Compression::new(lv));
    io::copy(&mut File::open(src)?, &mut enc)?;
    enc.finish()?;
    Ok(())
}

// --- 7z (system command) ---

fn compress_7z(inputs: &[PathBuf], output: &Path) -> Result<()> {
    let cmd = if which("7z") { "7z" } else if which("7za") { "7za" } else {
        bail!("7z compression requires 7z or 7za to be installed");
    };
    let mut args = vec!["a".to_string(), output.to_string_lossy().to_string()];
    args.extend(inputs.iter().map(|p| p.to_string_lossy().to_string()));
    let status = std::process::Command::new(cmd).args(&args).status()?;
    if !status.success() {
        bail!("7z command failed");
    }
    Ok(())
}

fn which(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn clamp(level: Option<u32>, min: u32, max: u32, default: u32) -> u32 {
    level.unwrap_or(default).clamp(min, max)
}
