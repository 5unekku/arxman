mod cli;
mod compress;
mod detect;
mod extract;
mod format;

use anyhow::bail;
use clap::Parser;
use cli::Args;

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if args.extract {
        extract::run(&args.files, args.format.as_deref())?;
    } else if args.compress {
        if args.files.len() < 2 {
            bail!("compress mode requires at least one input and an output file");
        }
        let (inputs, output) = args.files.split_at(args.files.len() - 1);
        compress::run(inputs, &output[0], args.format.as_deref(), args.level)?;
    } else {
        bail!("specify -x to extract or -c to compress");
    }

    Ok(())
}
