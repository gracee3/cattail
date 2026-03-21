use clap::{Parser, ValueEnum};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum PrefixMode {
    Basename,
    Relative,
    Full,
}

#[derive(Debug, Parser)]
#[command(name = "cattail", about = "Tail multiple files and glob patterns")]
struct Args {
    #[arg(short = 'n', long = "lines", default_value_t = 50)]
    lines: usize,

    #[arg(long = "interval-ms", default_value_t = 200)]
    interval_ms: u64,

    #[arg(long = "prefix", value_enum, default_value_t = PrefixMode::Basename)]
    prefix: PrefixMode,

    #[arg(long = "since-now", default_value_t = false)]
    since_now: bool,

    #[arg(long = "color", value_enum, default_value_t = ColorMode::Auto)]
    color: ColorMode,

    #[arg(required = true)]
    inputs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub lines: usize,
    pub interval_ms: u64,
    pub prefix: PrefixMode,
    pub since_now: bool,
    pub color: ColorMode,
    pub inputs: Vec<String>,
}

impl Config {
    pub fn parse() -> Self {
        Self::parse_from(std::env::args_os())
    }

    pub fn parse_from<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let args = Args::parse_from(args);
        Self {
            lines: args.lines,
            interval_ms: args.interval_ms,
            prefix: args.prefix,
            since_now: args.since_now,
            color: args.color,
            inputs: args.inputs,
        }
    }

    pub fn backlog_lines(&self) -> usize {
        if self.since_now {
            0
        } else {
            self.lines
        }
    }

    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_interval_and_prefix_mode() {
        let config = Config::parse_from([
            "cattail",
            "--interval-ms",
            "750",
            "--prefix",
            "full",
            "app.log",
        ]);

        assert_eq!(config.interval_ms, 750);
        assert_eq!(config.prefix, PrefixMode::Full);
        assert_eq!(config.inputs, vec!["app.log".to_string()]);
        assert_eq!(config.backlog_lines(), 50);
    }

    #[test]
    fn since_now_wins_over_backlog_lines() {
        let config = Config::parse_from(["cattail", "-n", "100", "--since-now", "app.log"]);

        assert!(config.since_now);
        assert_eq!(config.backlog_lines(), 0);
    }
}
