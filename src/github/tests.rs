use super::*;

#[tokio::test]
async fn clone() {
    let directory_path = "./Test/GitHubSearch".into();
    let _ = fs::remove_dir_all(&directory_path);
    let owner = "RobinCombrink".to_owned();
    let repo = "GitCloner".to_owned();
    let branch = "main".to_owned();

    let token = GitHubToken::try_from("a-token-a-public-clone-never-presents".to_owned())
        .expect("a non-empty value is a token");

    let cloner = GitCloner::new(owner.clone(), token, directory_path).unwrap();
    let directory_path = cloner.directory_path.clone();
    let progress_bar = create_download_asset_progress_bar(&owner, &repo, directory_path.clone());
    GitCloner::clone_repository(
        owner,
        repo,
        Some(branch.clone()),
        cloner.directory_path,
        cloner.account,
        progress_bar,
    )
    .expect("a public repository clones without presenting the token");

    let origin_url = Repository::open(directory_path.join(&branch))
        .expect("the clone lands in a directory named for its branch")
        .find_remote(REMOTE_NAME)
        .expect("the clone has an origin remote")
        .url()
        .map(str::to_owned);
    let _ = fs::remove_dir_all(&directory_path);

    assert_eq!(
        origin_url.as_deref(),
        Some("https://github.com/RobinCombrink/GitCloner")
    );
}
