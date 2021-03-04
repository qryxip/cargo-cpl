use anyhow::{anyhow, Context as _};
use camino::Utf8Path;
use cargo_metadata as cm;
use ignore::Walk;
use indexmap::{indexmap, IndexMap};
use maplit::hashset;
use serde::{de::Error as _, Deserialize, Deserializer};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};
use url::Url;

pub(crate) fn list_metadata(
    root: &Path,
) -> anyhow::Result<IndexMap<cm::PackageId, Rc<cm::Metadata>>> {
    let mut metadata_set = indexmap!();
    let visited = &mut hashset!();
    for manifest_path in manifest_paths(root)? {
        if visited.contains(&manifest_path) {
            continue;
        }
        let metadata = Rc::new(cargo_metadata(&manifest_path)?);
        for ws_member in &metadata.workspace_members {
            metadata_set.insert(ws_member.clone(), metadata.clone());
            visited.insert(PathBuf::from(&metadata[ws_member].manifest_path));
        }
    }
    return Ok(metadata_set);

    fn manifest_paths(root: &Path) -> Result<Vec<PathBuf>, ignore::Error> {
        Walk::new(root)
            .map(|e| e.map(ignore::DirEntry::into_path))
            .filter(|p| !matches!(p, Ok(p) if p.file_name() != Some("Cargo.toml".as_ref())))
            .collect()
    }
}

fn locate_project(cwd: &Path) -> anyhow::Result<PathBuf> {
    cwd.ancestors()
        .map(|p| p.join("Cargo.toml"))
        .find(|p| p.exists())
        .with_context(|| {
            format!(
                "could not find `Cargo.toml` in `{}` or any parent directory",
                cwd.display(),
            )
        })
}

fn cargo_metadata(manifest_path: &Path) -> anyhow::Result<cm::Metadata> {
    cm::MetadataCommand::new()
        .manifest_path(manifest_path)
        .exec()
        .map_err(|err| match err {
            cm::Error::CargoMetadata { stderr } => {
                anyhow!("{}", stderr.trim_start_matches("error: ").trim_end())
            }
            err => anyhow::Error::msg(err),
        })
}

pub(crate) trait PackageExt {
    fn metadata(&self) -> serde_json::Result<PackageMetadata>;
    fn manifest_dir(&self) -> &Utf8Path;
    fn lib_target(&self) -> Option<&cm::Target>;
    fn proc_macro_target(&self) -> Option<&cm::Target>;
    fn bin_target(&self, name: &str) -> anyhow::Result<&cm::Target>;
    fn has_lib_target(&self) -> bool {
        self.lib_target().is_some()
    }
    fn has_proc_macro_target(&self) -> bool {
        self.proc_macro_target().is_some()
    }
}

impl PackageExt for cm::Package {
    fn metadata(&self) -> serde_json::Result<PackageMetadata> {
        match self.metadata.clone() {
            serde_json::Value::Null => Ok(PackageMetadata::default()),
            metadata => serde_json::from_value(metadata),
        }
    }

    fn manifest_dir(&self) -> &Utf8Path {
        self.manifest_path
            .parent()
            .expect("should end with `Cargo.toml`")
    }

    fn lib_target(&self) -> Option<&cm::Target> {
        self.targets
            .iter()
            .find(|cm::Target { kind, .. }| *kind == ["lib".to_owned()])
    }

    fn proc_macro_target(&self) -> Option<&cm::Target> {
        self.targets
            .iter()
            .find(|cm::Target { kind, .. }| *kind == ["proc-macro".to_owned()])
    }

    fn bin_target(&self, name: &str) -> anyhow::Result<&cm::Target> {
        self.targets
            .iter()
            .find(|t| t.name == name && t.kind == ["bin".to_owned()])
            .with_context(|| format!("no bin target named `{}`", name))
    }
}

pub(crate) trait TargetExt {
    fn crate_name(&self) -> String;
    fn is_lib(&self) -> bool;
}

impl TargetExt for cm::Target {
    fn crate_name(&self) -> String {
        self.name.replace('-', "_")
    }

    fn is_lib(&self) -> bool {
        *self.kind == ["lib".to_owned()]
    }
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct PackageMetadata {
    #[serde(default)]
    pub(crate) cargo_compete: PackageMetadataCargoCompete,
}

#[derive(Deserialize, Default, Debug)]
pub(crate) struct PackageMetadataCargoCompete {
    #[serde(deserialize_with = "deserialize_bin")]
    pub(crate) bin: HashMap<String, Url>,
}

fn deserialize_bin<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<HashMap<String, Url>, D::Error> {
    let map = HashMap::<String, Value>::deserialize(deserializer)?;
    return Ok(map
        .into_iter()
        .map(|(key, Value { name, problem })| (name.unwrap_or(key), problem))
        .collect());

    #[derive(Deserialize)]
    struct Value {
        name: Option<String>,
        #[serde(deserialize_with = "deserialize_problem")]
        problem: Url,
    }

    fn deserialize_problem<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Url, D::Error> {
        return Problem::deserialize(deserializer)
            .map(|problem| match problem {
                Problem::Bare(url) | Problem::Field { url } => url,
            })
            .map_err(|_| D::Error::custom("expected `\"<url>\"` or `{ problem = \"<url>\"}`"));

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Problem {
            Bare(Url),
            Field { url: Url },
        }
    }
}
