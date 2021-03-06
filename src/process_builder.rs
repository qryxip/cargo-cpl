use anyhow::{anyhow, bail, Context as _};
use itertools::Itertools as _;
use maplit::btreemap;
use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
    process::{ExitStatus, Output, Stdio},
    str,
};

use crate::shell::Shell;

pub(crate) fn process(program: impl AsRef<OsStr>) -> ProcessBuilder<NotPresent> {
    ProcessBuilder {
        program: program.as_ref().to_owned(),
        args: vec![],
        cwd: (),
        env: btreemap!(),
    }
}

#[derive(Debug)]
pub(crate) struct ProcessBuilder<C: Presence<PathBuf>> {
    program: OsString,
    args: Vec<OsString>,
    env: BTreeMap<String, OsString>,
    cwd: C::Value,
}

impl<C: Presence<PathBuf>> ProcessBuilder<C> {
    pub(crate) fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    pub(crate) fn args(mut self, args: &[impl AsRef<OsStr>]) -> Self {
        self.args.extend(args.iter().map(|s| s.as_ref().to_owned()));
        self
    }

    pub(crate) fn env(mut self, key: &str, val: impl AsRef<OsStr>) -> Self {
        self.env.insert(key.to_owned(), val.as_ref().to_owned());
        self
    }

    pub(crate) fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<OsStr>,
    {
        self.env.extend(
            vars.into_iter()
                .map(|(k, v)| (k.as_ref().to_owned(), v.as_ref().to_owned())),
        );
        self
    }

    pub(crate) fn cwd(self, cwd: impl AsRef<Path>) -> ProcessBuilder<Present> {
        ProcessBuilder {
            program: self.program,
            args: self.args,
            cwd: cwd.as_ref().to_owned(),
            env: self.env,
        }
    }
}

impl ProcessBuilder<Present> {
    fn output(&self, check: bool, stdout: Stdio, stderr: Stdio) -> anyhow::Result<Output> {
        let output = std::process::Command::new(&self.program)
            .args(&self.args)
            .envs(&self.env)
            .current_dir(&self.cwd)
            .stdout(stdout)
            .stderr(stderr)
            .output()?;
        if check && !output.status.success() {
            bail!("{} didn't exit successfully: {}", self, output.status);
        }
        Ok(output)
    }

    pub(crate) fn exec(&self) -> anyhow::Result<()> {
        self.output(true, Stdio::inherit(), Stdio::inherit())?;
        Ok(())
    }

    pub(crate) fn exec_with_status(&self, shell: &mut Shell) -> anyhow::Result<()> {
        shell.status("Running", self)?;
        self.exec()
    }

    pub(crate) fn status_silent(&self) -> anyhow::Result<ExitStatus> {
        let Output { status, .. } = self.output(false, Stdio::null(), Stdio::null())?;
        Ok(status)
    }

    pub(crate) fn read(&self, check: bool) -> anyhow::Result<String> {
        let Output { stdout, .. } = self.output(check, Stdio::piped(), Stdio::inherit())?;
        let stdout =
            str::from_utf8(&stdout).map_err(|_| anyhow!("stream did not contain valid UTF-8"))?;
        Ok(stdout.trim_end().to_owned())
    }

    pub(crate) fn read_with_status(
        &self,
        check: bool,
        shell: &mut Shell,
    ) -> anyhow::Result<String> {
        shell.status("Running", self)?;
        self.read(check)
    }
}

impl fmt::Display for ProcessBuilder<Present> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            fmt,
            "`{}{}`",
            shell_escape::escape(self.program.to_string_lossy()),
            self.args.iter().format_with("", |arg, f| f(&format_args!(
                " {}",
                shell_escape::escape(arg.to_string_lossy()),
            ))),
        )
    }
}

pub(crate) trait Presence<T> {
    type Value;
}

#[derive(Debug)]
pub(crate) enum NotPresent {}

impl<T> Presence<T> for NotPresent {
    type Value = ();
}

#[derive(Debug)]
pub(crate) enum Present {}

impl<T> Presence<T> for Present {
    type Value = T;
}
