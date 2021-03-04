use anyhow::{anyhow, bail, ensure, Context as _};
use git2::{Branch, BranchType, Oid, Repository};
use std::borrow::Cow;
use url::Url;

pub(crate) fn remote(repo: &Repository) -> anyhow::Result<(String, String, String)> {
    let head = repo.head()?;
    ensure!(head.is_branch(), "`HEAD` is not a local branch");
    let local_branch_name = &Branch::wrap(head)
        .name()?
        .with_context(|| "the branch name is not a valid UTF-8")?
        .to_owned();
    let upstream_name = &repo
        .find_branch(local_branch_name, BranchType::Local)?
        .upstream()
        .and_then(|u| u.name().map(|name| name.unwrap_or_default().to_owned()))
        .with_context(|| "could not get find the upstream branch")?;
    let (remote_name, remote_branch_name) = match *upstream_name.split('/').collect::<Vec<_>>() {
        [remote_name, remote_branch_name] => (remote_name, remote_branch_name.to_owned()),
        _ => bail!("could not parse {:?}", upstream_name),
    };
    let remote_url = repo
        .find_remote(remote_name)
        .with_context(|| format!("`{}` is not a remote", upstream_name))?
        .url()
        .and_then(|url| url.parse::<Url>().ok())
        .with_context(|| "the remote URL is not a valid URL")?;
    ensure!(
        remote_url.host_str() == Some("github.com"),
        "expected GitHub, got `{}`, remote_url",
    );
    let (s1, s2) = match *remote_url.path().split('/').collect::<Vec<_>>() {
        [_, s1, s2] => (s1, s2),
        _ => bail!("expected 2 segments: `{}`", remote_url.path()),
    };
    let username = s1.to_owned();
    let repo_name = s2.trim_end_matches(".git").to_owned();
    Ok((username, repo_name, remote_branch_name))
}

pub(crate) fn rev(repo: &Repository) -> anyhow::Result<Oid> {
    Ok(repo.head()?.peel_to_commit()?.id())
}

fn percent_decode(segment: &str) -> anyhow::Result<String> {
    let decodor = || percent_encoding::percent_decode_str(segment);
    decodor()
        .decode_utf8()
        .map(Cow::into_owned)
        .map_err(|e| anyhow!("{}: {}", e, decodor().decode_utf8_lossy()))
}

fn second_path_segment(url: &Url) -> anyhow::Result<String> {
    let segments = url
        .path_segments()
        .map(|ss| ss.collect::<Vec<_>>())
        .unwrap_or_default();

    let segment = segments.get(1).with_context(|| {
        format!(
            "the number of path segments is {} but the index is 1: {}",
            segments.len(),
            url,
        )
    })?;

    let decodor = || percent_encoding::percent_decode_str(segment);

    decodor()
        .decode_utf8()
        .map(Cow::into_owned)
        .map_err(|e| anyhow!("{}: {}", e, decodor().decode_utf8_lossy()))
}
