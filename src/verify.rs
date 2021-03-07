use crate::{
    github, process_builder,
    shell::Shell,
    workspace::{self, PackageExt as _, TargetExt as _},
};
use anyhow::{anyhow, Context as _};
use camino::Utf8Path;
use cargo_metadata as cm;
use git2::Repository;
use indoc::indoc;
use itertools::Itertools as _;
use kuchiki::{traits::TendrilSink as _, ElementData, NodeDataRef, NodeRef};
use maplit::{btreemap, btreeset};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
};
use url::Url;

pub fn verify(
    nightly_toolchain: &str,
    open: bool,
    cwd: &Path,
    shell: &mut Shell,
) -> anyhow::Result<()> {
    let repo = &Repository::discover(cwd)?;
    let repo_workdir = repo.workdir().expect("this is constructed with `discover`");

    let (gh_username, gh_repo_name, gh_branch_name) = github::remote(repo)?;
    let rev = github::rev(repo)?;

    let gh_blob_url = |rel_filepath: &Utf8Path| -> anyhow::Result<Url> {
        let url = format!(
            "https://github.com/{}/{}/blob/{}/{}",
            gh_username,
            gh_repo_name,
            rev,
            rel_filepath.iter().format("/"),
        );
        url.parse()
            .with_context(|| format!("invalid URL: {:?}", url))
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
                (problem_url, gh_blob_url(Utf8Path::new(&relative_src_path))?)
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

    prepare_doc(
        open,
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
                let manifest_path_url = gh_blob_url(relative_manifest_path)?;
                let code_sizes = krate.is_lib().then(|| CodeSizes::new(krate));
                Ok(PackageAnalysis {
                    package,
                    krate,
                    relative_manifest_path,
                    manifest_path_url,
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
    relative_manifest_path: &'a Utf8Path,
    manifest_path_url: Url,
    code_sizes: Option<CodeSizes>,
    verifications: &'a BTreeSet<(&'a Url, Url)>,
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
}

fn prepare_doc(
    open: bool,
    analysis: &[PackageAnalysis<'_>],
    shell: &mut Shell,
) -> anyhow::Result<()> {
    let manifest = &mut indoc! {r#"
        [package]
        name = "__cargo_cpl_doc"
        version = "0.0.0"
        edition = "2018"

        [lib]
        name = "{ toc }"

        [dependencies]
    "#}
    .parse::<toml_edit::Document>()
    .unwrap();

    let dependencies = manifest["dependencies"].as_table_mut().unwrap();
    for PackageAnalysis { package, .. } in analysis {
        dependencies[&package.name] = {
            let mut tbl = toml_edit::InlineTable::default();
            tbl.get_or_insert("path", package.manifest_dir().as_str());
            toml_edit::value(tbl)
        };
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

    let mut lib_rs = "//! The table of contents.\n".to_owned();
    lib_rs += "//!\n";
    for line in toc.to_md().lines() {
        lib_rs += "//!";
        if !line.is_empty() {
            lib_rs += " ";
        }
        lib_rs += line;
        lib_rs += "\n";
    }

    let ws = &dirs_next::cache_dir()
        .with_context(|| "could not find the cache directory")?
        .join("cargo-cpl")
        .join("workspace");

    xshell::mkdir_p(ws.join("src"))?;
    xshell::rm_rf(ws.join("target").join("doc"))?;

    xshell::write_file(ws.join("Cargo.toml"), manifest.to_string())?;
    xshell::write_file(ws.join("src").join("lib.rs"), lib_rs)?;

    let cargo_exe = &process_builder::process("rustup")
        .args(&["which", "cargo"])
        .cwd(ws)
        .read(true)?;

    if Path::new(cargo_exe)
        .with_file_name("rustfmt")
        .with_extension(std::env::consts::EXE_EXTENSION)
        .exists()
    {
        process_builder::process(cargo_exe)
            .arg("fmt")
            .cwd(ws)
            .exec_with_status(shell)?;
    }

    let run_cargo_doc = |open: bool, shell: &mut Shell| -> _ {
        process_builder::process(cargo_exe)
            .arg("doc")
            .args(if open { &["--open"] } else { &[] })
            .cwd(ws)
            .exec_with_status(shell)
    };

    run_cargo_doc(false, shell)?;

    for analysis in analysis {
        let path = &ws
            .join("target")
            .join("doc")
            .join(analysis.krate.crate_name())
            .join("index.html");
        let index_html = xshell::read_file(path)?;
        let index_html = modify_index_html(&index_html, analysis)?;
        xshell::write_file(path, index_html)?;
        shell.status("Modified", path.display())?;
    }
    if open {
        run_cargo_doc(true, shell)?;
    }
    Ok(())
}

fn modify_index_html(html: &str, analysis: &PackageAnalysis<'_>) -> anyhow::Result<String> {
    let PackageAnalysis {
        package,
        krate: _,
        relative_manifest_path,
        manifest_path_url,
        code_sizes,
        verifications,
    } = analysis;

    let document = kuchiki::parse_html().one(html);

    let orig_fqn = document
        .select_first(".fqn")
        .ok()
        .with_context(|| "could not parse `index.html`: missing `.fqn`")?;

    let new_fqn = kuchiki::parse_html()
        .one(format!(
            indoc! {r#"
                <html>
                  <body>
                    <h1 class="fqn">
                      <span class="in-band">Package {} v{}</span>
                    </h1>
                  </body>
                </html>
            "#},
            package.name,
            v_htmlescape::escape(&package.version.to_string()),
        ))
        .select_first("body > h1")
        .unwrap();

    let new_div = kuchiki::parse_html()
        .one(format!(
            indoc! {r##"
                <html>
                  <body>
                    <div class="docblock">
                      <p>{}</p>
                      <ul>
                        <li>Manifest: <a href="{}"><code>{}</code></a></li>
                        <li>License: {}</li>
                      </ul>
                      {}
                      <h1 id="verified-with" class="section-header"><a href="#verified-with">Verified with</a></h1>
                      {}
                    </div>
                  </body>
                </html>
            "##},
            match verifications.len() {
                0 => format!("{} This library is not verified", WARNING),
                1 => format!("{} This library is verified with 1 solution", HEAVY_CHECK_MARK),
                n => format!("{} This library is verified with {} solutions", HEAVY_CHECK_MARK, n),
            },
            manifest_path_url,
            v_htmlescape::escape(relative_manifest_path.as_ref()).to_string(),
            if let Some(license) = &package.license {
                format!("<code>{}</code>", v_htmlescape::escape(license))
            } else {
                "<strong>missing license</strong>".to_owned()
            },
            code_sizes
                .as_ref()
                .map(|CodeSizes { unmodified }| {
                    format!(
                        indoc! {r##"
                            <h1 id="code-size" class="section-header"><a href="#code-size">Code size</a></h1>
                            <ul>
                              <li>unmodified: {}</li>
                            </ul>
                        "##},
                        match unmodified {
                            Ok(size) => {
                                let (div, rem) = (size / 1024, size % 1024);
                                format!(
                                    "{}.{} KiB + (not yet implemented) KiB",
                                    div, 10 * rem / 1024,
                                )
                            }
                            Err(err) => format!("<code>{}</code>", v_htmlescape::escape(err)),
                        },
                    )
                })
                .unwrap_or_default(),
            if verifications.is_empty() {
                "<strong>This library is not verified.</strong>".to_owned()
            } else {
                let mut ul = "<ul>".to_owned();
                for (problem_url, gh_blob_url) in *verifications {
                    ul += "<li>";
                    ul += &format!(
                        r#"<a href="{0}">{0}</a>"#,
                        v_htmlescape::escape(problem_url.as_ref()),
                    );
                    ul += " ";
                    ul += &format!(
                        r#"(<a href="{}">code</a>)"#,
                        v_htmlescape::escape(gh_blob_url.as_ref()),
                    );
                    ul += "</li>";
                }
                ul += "</ul>";
                ul
            },
        ))
        .select_first("body > div")
        .unwrap();

    orig_fqn.as_node().insert_before(new_fqn.as_node().clone());
    orig_fqn.as_node().insert_before(new_div.as_node().clone());
    orig_fqn.as_node().insert_before(hr());
    return Ok(document.to_string());

    fn hr() -> NodeRef {
        return HR.with(|hr| hr.as_node().clone());

        thread_local! {
            static HR: NodeDataRef<ElementData> = kuchiki::parse_html()
                .one(indoc! {"
                    <html>
                      <body>
                        <hr>
                      </body>
                    </html>
                "})
                .select_first("body > hr")
                .unwrap();
        }
    }
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
    }
}

static HEAVY_CHECK_MARK: &str = r#"<img src="https://github.githubassets.com/images/icons/emoji/unicode/2714.png" alt="✔" title="✔" width="20" height="20">"#;
static WARNING: &str = r#"<img src="https://github.githubassets.com/images/icons/emoji/unicode/26a0.png" alt="⚠" title="⚠" width="20" height="20">"#;
