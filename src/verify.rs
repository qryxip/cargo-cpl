use crate::{
    github, process_builder,
    shell::Shell,
    workspace::{self, PackageExt as _, TargetExt as _},
};
use anyhow::{anyhow, Context as _};
use camino::Utf8Path;
use cargo_metadata as cm;
use git2::Repository;
use ignore::Walk;
use indoc::indoc;
use itertools::Itertools as _;
use maplit::{btreemap, btreeset};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
};
use url::Url;

pub fn verify_for_gh_pages(
    nightly_toolchain: &str,
    open: bool,
    cwd: &Path,
    shell: &mut Shell,
) -> anyhow::Result<()> {
    let repo = &Repository::discover(cwd)?;
    let repo_workdir = repo.workdir().expect("this is constructed with `discover`");

    let (gh_username, gh_repo_name, gh_branch_name) = github::remote(repo)?;
    let rev = github::rev(repo)?;

    let gh_url = format!("https://github.com/{}/{}", gh_username, gh_repo_name);
    let gh_url = &gh_url
        .parse::<Url>()
        .with_context(|| format!("invalid URL: {}", gh_url))?;

    let gh_blob_url = |rel_filepath: &Utf8Path| -> Url {
        let mut url = gh_url.clone();
        let mut path_segments = url.path_segments_mut().expect("this is `https://`");
        path_segments.push("blob");
        path_segments.push(&rev.to_string());
        path_segments.extend(rel_filepath);
        drop(path_segments);
        url
    };

    let metadata_list = workspace::list_metadata(repo_workdir)?;

    let cargo_exes = metadata_list
        .values()
        .map(|m| &m.workspace_root)
        .unique()
        .map(|workspace_root| {
            let cargo_exe = process_builder::process("rustup")
                .args(&["which", "cargo"])
                .cwd(workspace_root)
                .read(true)?;
            Ok((workspace_root, cargo_exe))
        })
        .collect::<anyhow::Result<HashMap<_, _>>>()?;

    let bin_metadata = metadata_list
        .iter()
        .map(|(ws_member, metadata)| {
            let package_metadata = metadata[ws_member].metadata()?;
            Ok((ws_member, package_metadata.cargo_compete.bin))
        })
        .collect::<anyhow::Result<HashMap<_, _>>>()?;

    let mut verifications: BTreeMap<_, BTreeSet<_>> = btreemap!();

    for (ws_member, metadata) in &metadata_list {
        let ws_member = &metadata[ws_member];

        let normal_deps = &metadata
            .resolve
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .map(|cm::Node { id, deps, .. }| {
                let deps = deps
                    .iter()
                    .filter(|cm::NodeDep { dep_kinds, .. }| {
                        dep_kinds
                            .iter()
                            .any(|cm::DepKindInfo { kind, .. }| *kind == cm::DependencyKind::Normal)
                    })
                    .map(|cm::NodeDep { name, pkg, .. }| (name, pkg))
                    .collect::<Vec<_>>();
                (id, deps)
            })
            .collect::<HashMap<_, _>>();

        let explicit_names_in_toml = ws_member
            .dependencies
            .iter()
            .flat_map(|cm::Dependency { rename, .. }| rename.as_ref())
            .collect::<HashSet<_>>();

        let normal_deps_depth1 = &normal_deps[&ws_member.id]
            .iter()
            .flat_map(|&(name, pkg)| {
                let name_in_toml = if explicit_names_in_toml.contains(name) {
                    name
                } else {
                    &metadata[pkg].name
                };
                Some((name_in_toml, pkg))
            })
            .collect::<BTreeMap<_, _>>();

        for (bin_name, problem_url) in &bin_metadata[&ws_member.id] {
            let bin_target = ws_member.bin_target(bin_name)?;

            let verification = {
                let relative_src_path = dunce::canonicalize(&bin_target.src_path)
                    .ok()
                    .and_then(|p| p.strip_prefix(repo_workdir).ok().map(ToOwned::to_owned))
                    .with_context(|| {
                        format!(
                            "could not get the relative path of `{}`",
                            bin_target.src_path,
                        )
                    })?
                    .into_os_string()
                    .into_string()
                    .map_err(|_| {
                        anyhow!(
                            "`{}` was canonicalized to non UTF-8 string",
                            bin_target.src_path,
                        )
                    })?;
                (problem_url, gh_blob_url(Utf8Path::new(&relative_src_path)))
            };

            let cargo_udeps_output = &process_builder::process("rustup")
                .arg("run")
                .arg(nightly_toolchain)
                .arg("cargo")
                .arg("udeps")
                .arg("--manifest-path")
                .arg(&ws_member.manifest_path)
                .arg("--bin")
                .arg(bin_name)
                .arg("--output")
                .arg("json")
                .cwd(&metadata.workspace_root)
                .read_with_status(false, shell)?;

            let unused_normal_names_in_toml =
                serde_json::from_str::<CargoUdepsOutput>(cargo_udeps_output)?
                    .unused_deps
                    .into_iter()
                    .find(|(_, CargoUdepsOutputDeps { manifest_path, .. })| {
                        *manifest_path == ws_member.manifest_path
                    })
                    .map(|(_, CargoUdepsOutputDeps { normal, .. })| normal)
                    .unwrap_or_default();

            let deps_in_same_repo = {
                let mut deps = btreeset!();
                let stack = &mut normal_deps_depth1
                    .iter()
                    .filter(|&(name_in_toml, _)| {
                        !unused_normal_names_in_toml.contains(*name_in_toml)
                    })
                    .map(|(_, package_id)| *package_id)
                    .collect::<Vec<_>>();
                while let Some(package_id) = stack.pop() {
                    if deps.insert(package_id) {
                        stack.extend(normal_deps[package_id].iter().map(|(_, pkg)| *pkg));
                    }
                }
                deps.into_iter()
                    .flat_map(|id| {
                        let package = &metadata[id];
                        let cm::Target { src_path, .. } = &package
                            .lib_target()
                            .or_else(|| package.proc_macro_target())?;
                        match dunce::canonicalize(src_path) {
                            Ok(src_path) if src_path.starts_with(repo_workdir) => Some(Ok(id)),
                            Ok(_) => None,
                            Err(err) => Some(Err(err)),
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };

            for dep_in_same_repo in deps_in_same_repo {
                verifications
                    .entry(dep_in_same_repo)
                    .or_default()
                    .insert(verification.clone());
            }
        }
    }

    for ws_member in metadata_list.keys() {
        verifications.entry(ws_member).or_default();
    }

    for (ws_member, metadata) in &metadata_list {
        let ws_member = &metadata[ws_member];
        for bin_name in bin_metadata[&ws_member.id].keys() {
            process_builder::process(&cargo_exes[&metadata.workspace_root])
                .arg("compete")
                .arg("t")
                .arg("--manifest-path")
                .arg(&ws_member.manifest_path)
                .arg(bin_name)
                .cwd(&metadata.workspace_root)
                .exec_with_status(shell)?;
        }
    }

    let crate_names = metadata_list
        .values()
        .flat_map(|metadata| {
            metadata
                .workspace_members
                .iter()
                .map(move |id| &metadata[id])
                .flat_map(|package| {
                    let krate = package
                        .lib_target()
                        .or_else(|| package.proc_macro_target())?;
                    Some((&package.name, krate.crate_name()))
                })
        })
        .collect::<HashMap<_, _>>();

    prepare_doc(
        open,
        nightly_toolchain,
        repo_workdir,
        &verifications
            .iter()
            .flat_map(|(package_id, verifications)| {
                let package = &metadata_list[*package_id][package_id];
                let krate = package
                    .lib_target()
                    .or_else(|| package.proc_macro_target())?;
                Some((package, krate, verifications))
            })
            .map(|(package, krate, verifications)| {
                let relative_manifest_path = package
                    .manifest_path
                    .strip_prefix(repo_workdir)
                    .map_err(|_| {
                        anyhow!("`{}` is outside of the repository", package.manifest_path)
                    })?;
                let manifest_dir_blob_url = gh_blob_url(&relative_manifest_path.with_file_name(""));
                let dependency_ul = {
                    let metadata = &metadata_list[&package.id];
                    let crate_names = metadata
                        .workspace_members
                        .iter()
                        .map(move |id| &metadata[id])
                        .flat_map(|package| {
                            let krate = package
                                .lib_target()
                                .or_else(|| package.proc_macro_target())?;
                            Some((&*package.name, krate.crate_name()))
                        })
                        .collect::<HashMap<_, _>>();
                    package.dependency_ul(|k| crate_names.get(k).map(|v| &**v))?
                };
                let code_sizes = krate.is_lib().then(|| CodeSizes::new(krate));
                Ok(PackageAnalysis {
                    package,
                    krate,
                    git_url: gh_url,
                    relative_manifest_path,
                    manifest_dir_blob_url,
                    dependency_ul,
                    code_sizes,
                    verifications,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?,
        shell,
    )?;

    Ok(())
}

struct PackageAnalysis<'a> {
    package: &'a cm::Package,
    krate: &'a cm::Target,
    git_url: &'a Url,
    relative_manifest_path: &'a Utf8Path,
    manifest_dir_blob_url: Url,
    dependency_ul: Vec<(String, String)>,
    code_sizes: Option<CodeSizes>,
    verifications: &'a BTreeSet<(&'a Url, Url)>,
}

impl PackageAnalysis<'_> {
    fn to_html_header(&self) -> String {
        format!(
            indoc! {r##"
                <script>
                "use strict";

                registerModification(
                    {},
                    {},
                    {},
                    [{}],
                    {},
                    [{}],
                );

                {}</script>
            "##},
            json!(self.manifest_dir_blob_url),
            json!(self.package.license),
            json!(format!(
                "cargo add {} --git {}",
                self.package.name, self.git_url,
            )),
            self.dependency_ul
                .iter()
                .map(|(s, u)| json!([s, u]))
                .join(","),
            json!(self.code_sizes.as_ref().map(CodeSizes::unmodified)),
            self.verifications
                .iter()
                .map(|(u1, u2)| json!([u1, u2]))
                .join(","),
            include_str!("../injection/dist/index.js").trim_start_matches("\"use strict\";\n"),
        )
    }
}

struct CodeSizes {
    unmodified: Result<usize, String>,
}

impl CodeSizes {
    fn new(krate: &cm::Target) -> Self {
        match crate::rust::expand_mods(&krate.src_path) {
            Ok(code) => Self {
                unmodified: Ok(code.len()),
            },
            Err(err) => Self {
                unmodified: Err(err),
            },
        }
    }

    fn unmodified(&self) -> serde_json::Value {
        match &self.unmodified {
            Ok(n) => json!(n),
            Err(e) => json!(e),
        }
    }
}

trait PackageExt {
    fn dependency_ul<'a>(
        &self,
        crate_name: impl FnMut(&str) -> Option<&'a str>,
    ) -> anyhow::Result<Vec<(String, String)>>;
}

impl PackageExt for cm::Package {
    fn dependency_ul<'a>(
        &self,
        mut crate_name: impl FnMut(&str) -> Option<&'a str>,
    ) -> anyhow::Result<Vec<(String, String)>> {
        let Manifest { dependencies } = toml::from_str(&xshell::read_file(&self.manifest_path)?)?;

        let paths = dependencies
            .iter()
            .flat_map(|(name_in_toml, value)| match value {
                ManifestDependency::Version(_) => None,
                ManifestDependency::Braced { package, path, .. } => {
                    Some((package.as_ref().unwrap_or(name_in_toml), path.as_ref()?))
                }
            })
            .collect::<HashMap<_, _>>();

        let short_reqs = dependencies
            .iter()
            .flat_map(|(name_in_toml, value)| {
                let version = match value {
                    ManifestDependency::Version(version) => version,
                    ManifestDependency::Braced { version, .. } => version.as_ref()?,
                };
                let short_req = if version.chars().all(|c| matches!(c, '0'..='9' | '.')) {
                    format!("^{}", version)
                } else {
                    version.clone()
                };
                Some((name_in_toml, short_req))
            })
            .collect::<HashMap<_, _>>();

        return Ok(self
            .dependencies
            .iter()
            .filter(|cm::Dependency { kind, .. }| *kind == cm::DependencyKind::Normal)
            .map(
                |cm::Dependency {
                     name,
                     source,
                     req,
                     rename,
                     ..
                 }| {
                    if source.as_deref()
                        == Some("registry+https://github.com/rust-lang/crates.io-index")
                    {
                        let req = short_reqs
                            .get(rename.as_ref().unwrap_or(name))
                            .cloned()
                            .unwrap_or_else(|| req.to_string());
                        (
                            format!("{} {}", name, req),
                            format!("https://docs.rs/{}/{}", name, req),
                        )
                    } else if let Some(url) = source.as_ref().and_then(|s| s.strip_prefix("git+")) {
                        (format!("{} (git+{})", name, url), url.to_owned())
                    } else if let Some(source) = &source {
                        (format!("{} ({})", name, source), "".to_owned())
                    } else if let (Some(path), Some(crate_name)) =
                        (paths.get(name), crate_name(name))
                    {
                        (
                            format!("{} (path+{})", name, path),
                            format!("../{}/index.html", crate_name),
                        )
                    } else {
                        (format!("{} (unknown)", name), "".to_owned())
                    }
                },
            )
            .collect());

        #[derive(Deserialize)]
        struct Manifest {
            #[serde(default)]
            dependencies: HashMap<String, ManifestDependency>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ManifestDependency {
            Version(String),
            Braced {
                package: Option<String>,
                path: Option<String>,
                version: Option<String>,
            },
        }
    }
}

trait DependencyExt {
    fn to_list_item(&self) -> Option<(String, String)>;
}

impl DependencyExt for cm::Dependency {
    fn to_list_item(&self) -> Option<(String, String)> {
        (self.kind == cm::DependencyKind::Normal).then(|| ())?;
        Some(
            if self.source.as_deref()
                == Some("registry+https://github.com/rust-lang/crates.io-index")
            {
                (
                    format!("{} {}", self.name, self.req),
                    format!("https://docs.rs/{}/{}", self.name, self.req),
                )
            } else if let Some(url) = self.source.as_ref().and_then(|s| s.strip_prefix("git+")) {
                (format!("{} (git+{})", self.name, url), url.to_owned())
            } else if let Some(source) = &self.source {
                (format!("{} ({})", self.name, source), "".to_owned())
            } else {
                //(self.name.clone(), format!("../{}/index.html", self.name))
                todo!();
            },
        )
    }
}

fn prepare_doc(
    open: bool,
    nightly_toolchain: &str,
    repo_workdir: &Path,
    analysis: &[PackageAnalysis<'_>],
    shell: &mut Shell,
) -> anyhow::Result<()> {
    let manifest = &mut indoc! {r#"
        [workspace]
        members = []

        [package]
        name = "__cargo_cpl_doc"
        version = "0.0.0"
        edition = "2018"

        [lib]
        name = "__TOC"
    "#}
    .parse::<toml_edit::Document>()
    .unwrap();

    for PackageAnalysis {
        relative_manifest_path,
        ..
    } in analysis
    {
        let dst = Utf8Path::new(".")
            .join("copy")
            .join(relative_manifest_path)
            .with_file_name("");

        manifest["workspace"]["members"]
            .as_array_mut()
            .unwrap()
            .push(dst.as_str())
            .unwrap();
    }

    let toc = &mut TableOfContents::default();
    for PackageAnalysis {
        krate,
        relative_manifest_path,
        verifications,
        ..
    } in analysis
    {
        toc.insert(
            relative_manifest_path,
            &krate.crate_name(),
            !verifications.is_empty(),
        );
    }

    let mut lib_rs = "//! # Table of contents\n".to_owned();
    lib_rs += "//!\n";
    for line in toc.to_md().lines() {
        lib_rs += "//!";
        if !line.is_empty() {
            lib_rs += " ";
        }
        lib_rs += line;
        lib_rs += "\n";
    }
    lib_rs += "\n//! # As `[dependencies]`\n//!\n//! ```toml\n";
    for PackageAnalysis {
        package, git_url, ..
    } in analysis
    {
        lib_rs += &format!("//! {} = {{ git = \"{}\" }}\n", package.name, git_url);
    }
    lib_rs += "//! ```\n";

    let ws = &dirs_next::cache_dir()
        .with_context(|| "could not find the cache directory")?
        .join("cargo-cpl")
        .join("workspace");

    xshell::mkdir_p(ws.join(".cargo"))?;
    xshell::mkdir_p(ws.join("src"))?;
    xshell::rm_rf(ws.join("copy"))?;
    xshell::rm_rf(ws.join("target").join("doc"))?;

    xshell::write_file(ws.join(".cargo").join("config.toml"), CONFIG_TOML)?;
    xshell::write_file(ws.join("Cargo.toml"), manifest.to_string())?;
    xshell::write_file(ws.join("src").join("lib.rs"), lib_rs)?;

    for result in Walk::new(repo_workdir) {
        let from = &result?.into_path();
        if !from.is_file() {
            continue;
        }
        if from.file_name() == Some("Cargo.toml".as_ref())
            && !analysis
                .iter()
                .any(|PackageAnalysis { package, .. }| package.manifest_path == *from)
        {
            shell.status("Skipping", format!("Copying {}", from.display()))?;
            continue;
        }
        if let Ok(rel_path) = from.strip_prefix(repo_workdir) {
            if let Some(rel_path) = rel_path.to_str() {
                let to = &ws.join("copy").join(rel_path);
                xshell::mkdir_p(to.with_file_name(""))?;
                xshell::cp(from, to)?;
                shell.status(
                    "Copied",
                    format!("`{}` to `{}`", from.display(), to.display()),
                )?;
            }
        }
    }

    if process_builder::process("rustup")
        .args(&["which", "cargo-fmt", "--toolchain", nightly_toolchain])
        .cwd(ws)
        .status_silent()?
        .success()
    {
        process_builder::process("rustup")
            .args(&["run", nightly_toolchain, "cargo", "fmt"])
            .cwd(ws)
            .exec_with_status(shell)?;
    }

    let run_cargo_doc = |p: &str, open: bool, rustdocflags: Option<&str>, shell: &mut Shell| -> _ {
        process_builder::process("rustup")
            .args(&[
                "run",
                nightly_toolchain,
                "cargo",
                "doc",
                "-p",
                p,
                "--no-deps",
                "-Zrustdoc-map",
            ])
            .args(if open { &["--open"] } else { &[] })
            .envs(rustdocflags.map(|v| ("RUSTDOCFLAGS", v)))
            .cwd(ws)
            .exec_with_status(shell)
    };

    for analysis in analysis {
        xshell::write_file(ws.join("header.html"), analysis.to_html_header())?;
        run_cargo_doc(
            &analysis.package.name,
            false,
            Some("--html-in-header ./header.html"),
            shell,
        )?;
    }
    run_cargo_doc("__cargo_cpl_doc", open, None, shell)?;
    return Ok(());

    static CONFIG_TOML: &str = indoc! {r#"
        [doc.extern-map.registries]
        crates-io = "https://docs.rs/"
    "#};
}

#[derive(Debug, Deserialize)]
struct CargoUdepsOutput {
    unused_deps: BTreeMap<String, CargoUdepsOutputDeps>,
}

#[derive(Debug, Deserialize)]
struct CargoUdepsOutputDeps {
    manifest_path: PathBuf,
    normal: BTreeSet<String>,
}

#[derive(Default)]
struct TableOfContents {
    crates: BTreeMap<String, bool>,
    children: BTreeMap<String, Self>,
}

impl TableOfContents {
    fn insert(&mut self, relative_manifest_path: &Utf8Path, crate_name: &str, is_verified: bool) {
        let category = &mut relative_manifest_path
            .parent()
            .unwrap()
            .iter()
            .take(relative_manifest_path.iter().count().saturating_sub(2))
            .map(ToOwned::to_owned);

        let mut entry = self;
        for category in category {
            entry = entry.children.entry(category).or_default();
        }
        entry.crates.insert(crate_name.to_owned(), is_verified);
    }

    fn to_md(&self) -> String {
        let mut ret = "".to_owned();
        to_md(self, 0, &mut ret);
        return ret;

        fn to_md(this: &TableOfContents, depth: usize, ret: &mut String) {
            for (crate_name, is_verified) in &this.crates {
                *ret += &" ".repeat(4 * depth);
                *ret += "- ";
                *ret += if *is_verified {
                    HEAVY_CHECK_MARK
                } else {
                    WARNING
                };
                *ret += " ";
                *ret += "[";
                *ret += crate_name;
                *ret += "](../";
                *ret += crate_name;
                *ret += "/index.html)\n";
            }
            for (category, children) in &this.children {
                *ret += &" ".repeat(4 * depth);
                *ret += "- 📁 ";
                *ret += category;
                *ret += "\n";
                to_md(children, depth + 1, ret);
            }
        }

        static HEAVY_CHECK_MARK: &str = r#"<img src="https://github.githubassets.com/images/icons/emoji/unicode/2714.png" alt="✔" title="✔" width="20" height="20">"#;
        static WARNING: &str = r#"<img src="https://github.githubassets.com/images/icons/emoji/unicode/26a0.png" alt="⚠" title="⚠" width="20" height="20">"#;
    }
}
