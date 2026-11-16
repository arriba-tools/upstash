mod cli;
mod common;
mod upgrade;
mod vector;

use clap::Parser;

use cli::{Cli, Command};
use common::{Io, Stop};

fn main() {
    // Exit quietly when stdout closes early, as when piped into head, instead of panicking.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();
    let (stdout, stderr) = (std::io::stdout(), std::io::stderr());
    let (mut out, mut err) = (stdout.lock(), stderr.lock());
    let mut io = Io {
        out: &mut out,
        err: &mut err,
    };
    let result = match &cli.command {
        Command::Vector(command) => {
            vector::connect().and_then(|mut index| vector::run(&mut index, command, &mut io))
        }
        Command::Upgrade => upgrade::upgrade(&mut io),
    };
    let code = finish(result, &mut io);
    let _ = io.out.flush();
    std::process::exit(code);
}

/// The exit status of a run, saying on stderr why one stopped.
fn finish(result: Result<i32, Stop>, io: &mut Io) -> i32 {
    match result {
        Ok(code) => code,
        Err(stop) => {
            let _ = writeln!(io.err, "{stop}");
            stop.code()
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Run a `vector` command line against `index`, returning its exit status, output and errors.
    pub fn invoke_index(index: &mut dyn vector::Index, args: &[&str]) -> (i32, String, String) {
        let cli = match Cli::try_parse_from(
            std::iter::once("upstash-arriba").chain(args.iter().copied()),
        ) {
            Ok(cli) => cli,
            Err(e) => return (e.exit_code(), String::new(), e.to_string()),
        };
        let Command::Vector(command) = &cli.command else {
            panic!("not a vector command: {args:?}");
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = {
            let mut io = Io {
                out: &mut out,
                err: &mut err,
            };
            let result = vector::run(index, command, &mut io);
            finish(result, &mut io)
        };
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }
}
