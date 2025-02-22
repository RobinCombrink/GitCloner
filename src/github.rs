use anyhow::{anyhow, Context, Result};
use git2::{build::RepoBuilder, Cred, FetchOptions, RemoteCallbacks};
use indicatif::{ProgressBar, ProgressFinish, ProgressStyle};
use octocrab::Octocrab;
use secrecy::{ExposeSecret, SecretString};
use std::{borrow::Cow, fs, path::PathBuf, time::Duration};

use github_authentication::authentication::Authentication;

#[derive(Debug)]
pub struct GitCloner<T: Authentication> {
    authentication: T,
    directory_path: PathBuf,
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
    pub async fn clone_repository(
        &self,
        owner: &String,
        repo: &String,
        progress_bar: ProgressBar,
    ) -> Result<()> {
        let token = &self.authentication.get_token();

        let repository = octocrab::instance()
            .repos(owner, repo)
            .get()
            .await
            .expect("Invalid repo");

        println!("clone dir: {:#?}", &self.directory_path);
        fs::create_dir_all(&self.directory_path)
            .with_context(|| format!("Could not create directory: {:#?}", &self.directory_path))?;

        let directory_path = self.directory_path.join(repo.clone());

        let local_repo = git2::Repository::open(&directory_path);
        match local_repo {
            Ok(local_repo) => local_repo
                .find_remote("origin")
                .expect("Imagine not using origin as your remote name")
                .fetch(&["main"], None, None)
                .with_context(|| {
                    format!(
                        "Could not fetch origin main for local repository: {}",
                        repository.name
                    )
                }),
            Err(_) => {
                let url = &repository.html_url.ok_or(anyhow!(
                    "{} does not have an html url",
                    repository
                        .full_name
                        .unwrap_or_else(|| repository.name.clone())
                ))?;
                let username = self.authentication.get_username();
                let fetch_options =
                    Self::create_repository_fetch_options(&token, &username, progress_bar);

                let result = match RepoBuilder::new()
                    .fetch_options(fetch_options)
                    .clone(url.as_str(), &directory_path)
                    .with_context(|| {
                        format!("Failed to clone repo:\n{url}\n into {:?}", directory_path)
                    }) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let _ = fs::remove_dir_all(&directory_path);
                        Err(e)
                    }
                };
                result
            }
        }
    }

    pub fn create_repository_fetch_options<'token>(
        token: &'token SecretString,
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

        callbacks.credentials(move |_url, username_from_url, _allowed_types| {
            Cred::userpass_plaintext(
                username_from_url.unwrap_or_else(|| &default_username),
                token.expose_secret(),
            )
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

    #[tokio::test]
    async fn clone() {
        let _ = fs::remove_dir_all("./Test");
        let directory_path = "./Test".into();
        let owner = &"RobinCombrink".to_owned();
        let repo = &"GitCloner".to_owned();

        let authentication = GitHubCliAuthentication::new(owner.to_owned()).unwrap();

        let cloner = GitCloner::new(authentication, directory_path).unwrap();
        let directory_path = cloner.directory_path.clone();
        assert!(cloner
            .clone_repository(
                owner,
                repo,
                create_download_asset_progress_bar(owner, repo, directory_path,)
            )
            .await
            .is_ok());
        let _ = fs::remove_dir_all("./Test");
    }
}
