use anyhow::{Context, Result};
use git2::{build::RepoBuilder, Config, Cred, FetchOptions, RemoteCallbacks};
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use octocrab::Octocrab;
use secrecy::ExposeSecret;
use std::{borrow::Cow, fs, path::PathBuf, time::Duration};
use tokio::task::JoinSet;

use github_authentication::authentication::Authentication;

const REMOTE_NAME: &str = "origin";

pub struct GitClone {
    owner: String,
    repo: String,
    branch: String,
}

impl GitClone {
    pub fn new(owner: String, repo: String, branch: Option<String>) -> Self {
        Self {
            owner,
            repo,
            branch: branch.unwrap_or_else(|| "main".to_owned()),
        }
    }
}

#[derive(Debug)]
pub struct GitCloner<T: Authentication> {
    pub authentication: T,
    pub directory_path: PathBuf,
}

impl<T: Authentication> GitCloner<T> {
    pub fn new(authentication: T, directory_path: PathBuf) -> Result<GitCloner<T>> {
        let cloner = GitCloner {
            authentication,
            directory_path,
        };
        Self::initialise_octocrab(&cloner)?;
        Ok(cloner)
    }

    fn fetch_repository(
        repo: git2::Repository,
        branch: String,
        username: String,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        let mut remote = repo.find_remote(REMOTE_NAME)?;
        let mut fetch_options = Self::create_repository_fetch_options(&username, progress_bar);
        let result = remote.fetch(&[branch], Some(&mut fetch_options), None);
        if let Err(err) = result {
            if err.message().to_ascii_lowercase() == "no error" {
                return Ok(());
            } else {
                return Err(err.into());
            }
        }
        Ok(())
    }

    pub fn clone_repository(
        owner: String,
        repo: String,
        branch: String,
        directory_path: PathBuf,
        username: String,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        fs::create_dir_all(&directory_path)
            .with_context(|| format!("Could not create directory: {:#?}", &directory_path))?;

        let directory_path = directory_path.join(&repo).join(&branch);

        let url = format!("https://github.com/{owner}/{repo}");
        let fetch_options = Self::create_repository_fetch_options(&username, progress_bar);

        let result = RepoBuilder::new()
            .fetch_options(fetch_options)
            .clone(url.as_str(), &directory_path)
            .with_context(|| format!("Failed to clone repo:\n{url}\n into {:?}", directory_path));
        if let Err(_) = result {
            let _ = fs::remove_dir_all(&directory_path);
        }
        result?;
        Ok(())
    }

    pub async fn clone_or_fetch_repositories(&self, git_clones: Vec<GitClone>) -> Vec<Result<()>> {
        let multi_progress = MultiProgress::new();
        let mut tasks = JoinSet::new();

        let coordinator_progress_bar = create_progress_bar(
            git_clones.len(),
            format!("Updating {} repositories", git_clones.len(),),
            ProgressFinish::WithMessage(Cow::from(format!("{} repos updating", git_clones.len(),))),
            ProgressStyle::with_template(&format!(
                "[{{elapsed_precise}}] {{bar:{}.cyan/blue}} {{pos:>7}}/{{len:7}} {{msg}}",
                git_clones.len(),
            ))
            .unwrap()
            .progress_chars("✓▢▢"),
        );

        let coordinator_progress_bar = multi_progress.add(coordinator_progress_bar);
        coordinator_progress_bar.set_position(0);

        for repo_details in git_clones {
            let owner = repo_details.owner;
            let repo = repo_details.repo;
            let branch = repo_details.branch;
            let directory_path = self.directory_path.clone();
            let repository_path = directory_path.join(&repo);
            let username = self.authentication.get_username();
            let progress_bar = multi_progress.add(create_download_asset_progress_bar(
                &owner,
                &repo,
                directory_path.join(repo.clone()),
            ));

            let local_path = self.directory_path.join(&repo).join(&branch);

            match git2::Repository::open(&local_path) {
                Ok(local_repo) => tasks.spawn(async {
                    Self::fetch_repository(local_repo, branch, username, progress_bar)
                }),
                Err(_) => tasks.spawn(async {
                    Self::clone_repository(
                        owner,
                        repo,
                        branch,
                        directory_path,
                        username,
                        progress_bar,
                    )
                }),
            };
        }
        let mut results = Vec::new();
        while let Some(res) = tasks.join_next().await {
            match res {
                Ok(a) => results.push(a),
                Err(err) => results.push(Err(err.into())),
            }
            coordinator_progress_bar.set_position(results.len().try_into().unwrap());
        }
        results
    }

    pub fn create_repository_fetch_options<'token>(
        default_username: &'token str,
        progress_bar: ProgressBar,
    ) -> FetchOptions<'token> {
        let mut callbacks = RemoteCallbacks::new();

        let mut last_logged_progress = 0;
        callbacks.transfer_progress(move |progress| {
            let progress_percent = ((progress.received_objects() as f64
                / progress.total_objects() as f64)
                * 100 as f64)
                .ceil() as u64;
            let should_update_position = progress_percent != last_logged_progress
                && (progress_percent == 0 || progress_percent % 5 == 0);

            if should_update_position {
                progress_bar.set_position(progress_percent);
                last_logged_progress = progress_percent;
            }
            if progress_percent >= 100 {
                progress_bar.finish_using_style();
            }
            true
        });

        callbacks.credentials(move |url, _, _allowed_types| {
            let config = Config::open_default().expect("No git config");
            Cred::credential_helper(&config, url, Some(default_username))
        });

        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);
        fetch_options.depth(1);

        return fetch_options;
    }

    pub fn initialise_octocrab(&self) -> Result<()> {
        let token = self.authentication.get_token().expose_secret().to_owned();
        let instance = Octocrab::builder().personal_token(token).build()?;
        octocrab::initialise(instance);
        Ok(())
    }
}

fn create_progress_bar(
    length: usize,
    message: impl Into<Cow<'static, str>>,
    finish: ProgressFinish,
    style: ProgressStyle,
) -> ProgressBar {
    ProgressBar::new(length.try_into().unwrap())
        .with_finish(finish)
        .with_message(message)
        .with_style(style)
        .with_elapsed(Duration::new(0, 0))
        .with_position(0)
}

pub fn create_download_application_progress_bar() -> ProgressBar {
    ProgressBar::new(0).with_style(ProgressStyle::default_bar()
                 .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}")
                 .unwrap()
                 .progress_chars("#>-"))
}

pub fn create_download_asset_progress_bar(
    owner: &String,
    repo: &String,
    repository_path: PathBuf,
) -> ProgressBar {
    let finish = indicatif::ProgressFinish::WithMessage(Cow::from(format!(
        "Cloned {owner}/{repo} into {:#?}",
        repository_path
    )));

    let message = format!("Cloning {owner}/{repo} into {:#?}", repository_path);

    let style = ProgressStyle::with_template(
        "[{elapsed_precise}] {bar:100.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
    .unwrap()
    .progress_chars("##-");

    create_progress_bar(100, message, finish, style)
}

#[cfg(test)]
mod tests {
    use github_authentication::authentication::GitHubCliAuthentication;

    use super::*;

    #[test]
    fn clone() {
        let directory_path = "./Test/GitHubSearch".into();
        let _ = fs::remove_dir_all(&directory_path);
        let owner = "RobinCombrink".to_owned();
        let repo = "GitCloner".to_owned();
        let branch = "main".to_owned();

        let authentication = GitHubCliAuthentication::new(owner.clone()).unwrap();

        let cloner = GitCloner::new(authentication, directory_path).unwrap();
        let directory_path = cloner.directory_path.clone();
        let progress_bar =
            create_download_asset_progress_bar(&owner, &repo, directory_path.clone());
        assert!(GitCloner::<GitHubCliAuthentication>::clone_repository(
            owner,
            repo,
            branch,
            cloner.directory_path,
            cloner.authentication.get_username(),
            progress_bar
        )
        .is_ok());
        let _ = fs::remove_dir_all(&directory_path);
    }
}
