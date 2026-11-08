mod effects;
mod prompt;
mod terminal;

use std::{env, io, process::ExitCode, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    Assemble,
    Shimmer,
}

const HELP: &str = "Usage: shimmer <assemble|shimmer> <expanded-prompt>\n       shimmer --version\n       shimmer --help\n\nThe prompt is literal UTF-8 text. Run from an interactive terminal.\n";

fn run(effect: Effect, prompt: &str) -> io::Result<()> {
    let mut terminal = terminal::Terminal::open()?;
    let viewport = terminal.viewport();
    let layout = if effect == Effect::Assemble {
        let origin = terminal.native_origin();
        Some(
            prompt::Layout::parse(prompt, viewport, origin).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "prompt does not fit the terminal",
                )
            })?,
        )
    } else {
        None
    };
    let mut output = Vec::with_capacity(viewport.area() * 32);
    let mut assembly = layout
        .as_ref()
        .map(|layout| effects::Assembly::new(layout, viewport));
    terminal.enter(effect, layout.as_ref().map(|layout| layout.placement))?;
    if effect == Effect::Shimmer {
        terminal.wait_for(Duration::from_millis(100))?;
        effects::shimmer(&terminal, &mut output)?;
    } else if let (Some(assembly), Some(layout)) = (assembly.as_mut(), layout.as_ref()) {
        assembly.run(layout, &terminal, &mut output)?;
    }
    terminal.wait_for(Duration::from_millis(150))
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!(
            "shimmer {}",
            option_env!("SHIMMER_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
        );
        return ExitCode::SUCCESS;
    }
    if args.len() == 1 && args[0] == "--help" {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    let parsed = match args.as_slice() {
        [effect, prompt] => {
            let effect = match effect.to_str() {
                Some("assemble") => Some(Effect::Assemble),
                Some("shimmer") => Some(Effect::Shimmer),
                _ => None,
            };
            effect.zip(prompt.to_str())
        }
        _ => None,
    };
    let Some((effect, prompt)) = parsed else {
        eprint!("{HELP}");
        return ExitCode::from(2);
    };
    match run(effect, prompt) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) if terminal::cancelled() => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("shimmer: {error}");
            ExitCode::FAILURE
        }
    }
}
