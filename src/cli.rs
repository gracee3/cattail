use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Parser)]
#[command(name = "cattail", about = "Tail multiple files and glob patterns")]
struct Args {
    #[arg(short = 'n', long = "lines", default_value_t = 50)]
    lines: usize,

    #[arg(long = "color", value_enum, default_value_t = ColorMode::Auto)]
    color: ColorMode,

    #[arg(required = true)]
    inputs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub lines: usize,
    pub color: ColorMode,
    pub inputs: Vec<String>,
}

impl Config {
    pub fn parse() -> Self {
        let args = Args::parse();
        Self {
            lines: args.lines,
            color: args.color,
            inputs: args.inputs,
        }
    }
}
