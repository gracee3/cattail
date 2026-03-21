use anyhow::{Context, Result};
use clap_complete::{generate_to, shells::Bash, shells::Fish, shells::Zsh};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "all".to_string());
    let out_dir = args.next().unwrap_or_else(|| "packaging".to_string());
    let out_dir = PathBuf::from(out_dir);

    match command.as_str() {
        "man" => {
            let path = generate_man(&out_dir)?;
            println!("{}", path.display());
        }
        "completions" => {
            let paths = generate_completions(&out_dir)?;
            for path in paths {
                println!("{}", path.display());
            }
        }
        "all" => {
            let man = generate_man(&out_dir)?;
            let completions = generate_completions(&out_dir)?;
            println!("{}", man.display());
            for path in completions {
                println!("{}", path.display());
            }
        }
        other => {
            anyhow::bail!("unknown command: {other}");
        }
    }

    Ok(())
}

fn generate_man(out_dir: &Path) -> Result<PathBuf> {
    let man_dir = out_dir.join("man");
    fs::create_dir_all(&man_dir).context("creating man output directory")?;
    let command = cattail::cli::command();
    let path = man_dir.join("cattail.1");
    let file = fs::File::create(&path).context("creating man page")?;
    clap_mangen::Man::new(command)
        .render(&mut std::io::BufWriter::new(file))
        .context("rendering man page")?;
    Ok(path)
}

fn generate_completions(out_dir: &Path) -> Result<Vec<PathBuf>> {
    let comp_dir = out_dir.join("completions");
    fs::create_dir_all(&comp_dir).context("creating completion output directory")?;
    let mut command = cattail::cli::command();
    let paths = vec![
        generate_to(Bash, &mut command, "cattail", &comp_dir)?,
        generate_to(Zsh, &mut command, "cattail", &comp_dir)?,
        generate_to(Fish, &mut command, "cattail", &comp_dir)?,
    ];
    Ok(paths)
}
