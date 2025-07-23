use clap::Parser;
use anyhow::Result;
use varmap::{Args, run};

fn main() -> Result<()> {
    let args = Args::parse();
    run(args)?;
    Ok(())
}