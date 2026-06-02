use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "arx", about = "fast archive manager — extract, compress, detect")]
pub struct Args {
    /// extract archive(s)
    #[arg(short = 'x', conflicts_with = "compress")]
    pub extract: bool,

    /// compress files into archive (last positional is the output file)
    #[arg(short = 'c', conflicts_with = "extract")]
    pub compress: bool,

    /// format override (e.g. tar.gz, zip, zst, 7z)
    #[arg(short = 'f', value_name = "FMT")]
    pub format: Option<String>,

    /// compression level (range depends on format)
    #[arg(short = 'l', value_name = "LEVEL")]
    pub level: Option<u32>,

    /// always extract bare (no wrapper subfolder)
    #[arg(short = 'b', conflicts_with_all = ["compress", "subfolder"])]
    pub bare: bool,

    /// always wrap in a subfolder named after the archive
    #[arg(short = 's', conflicts_with_all = ["compress", "bare"])]
    pub subfolder: bool,

    /// files/archives and optional destination or output path
    #[arg(trailing_var_arg = true)]
    pub files: Vec<PathBuf>,
}
