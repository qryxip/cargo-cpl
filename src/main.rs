use anyhow::Context as _;
use cargo_cpl::Shell;
use std::{env, process};
use structopt::{
    clap::{self, AppSettings},
    StructOpt,
};
use termcolor::{Color, ColorSpec, WriteColor};

#[derive(Debug, StructOpt)]
#[structopt(
    about,
    author,
    bin_name("cargo"),
    global_settings(&[AppSettings::DeriveDisplayOrder, AppSettings::UnifiedHelpMessage])
)]
enum Opt {
    #[structopt(about, author)]
    Cpl(OptCpl),
}

#[derive(Debug, StructOpt)]
enum OptCpl {
    Verify {
        /// Open the docs in a browwer after the operation
        #[structopt(long)]
        open: bool,

        /// `nightly` toolchain
        #[structopt(long, value_name("TOOLCHAIN"), default_value("nightly"))]
        toolchain: String,
    },
}

fn main() {
    let Opt::Cpl(opt) = &Opt::from_args();
    let shell = &mut Shell::new();
    let result = (|| {
        let cwd = &env::current_dir().with_context(|| "could not get the CWD")?;
        match opt {
            OptCpl::Verify { open, toolchain } => cargo_cpl::verify(toolchain, *open, cwd, shell),
        }
    })();
    if let Err(err) = result {
        exit_with_error(err, shell.err());
    }
}

fn exit_with_error(err: anyhow::Error, mut wtr: impl WriteColor) -> ! {
    if let Some(err) = err.downcast_ref::<clap::Error>() {
        err.exit();
    }

    let mut bold_red = ColorSpec::new();
    bold_red
        .set_reset(false)
        .set_bold(true)
        .set_fg(Some(Color::Red));

    let _ = wtr.set_color(&bold_red);
    let _ = write!(wtr, "error:");
    let _ = wtr.reset();
    let _ = writeln!(wtr, " {}", err);

    for cause in err.chain().skip(1) {
        let _ = writeln!(wtr);
        let _ = wtr.set_color(&bold_red);
        let _ = write!(wtr, "Caused by:");
        let _ = wtr.reset();
        let _ = writeln!(wtr, "\n  {}", cause);
    }

    let _ = wtr.flush();

    process::exit(1);
}
