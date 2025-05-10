use clap::{Arg, Command};
use std::error::Error;
use std::process;
mod common;
mod env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let matches = Command::new("armory-downloader")
        .version("1.0")
        .about("Downloads files from Armory repositories")
        .arg(Arg::new("url")
            .help("The URL to download from")
            .required(true)
            .index(1))
        .arg(Arg::new("output")
            .short('o')
            .long("output")
            .help("Output file name")
            .takes_value(true))
        .get_matches();

    let url = matches.value_of("url").unwrap();
    let save_name = matches.value_of("output");

    let mut token = String::new();
    if let Ok(repo) = common::parse_repo_url(url) {
        match env::load_armory_configuration(&repo) {
            Ok(config) => {
                match common::get_user_token_of_armory(&repo, &config.username, &config.password).await {
                    Ok(t) => token = t,
                    Err(e) => {
                        eprintln!("\x1b[31mFailed to get token: {}\x1b[0m", e);
                        eprintln!("\x1b[33mPlease check your credentials and try again\x1b[0m");
                        process::exit(1);
                    }
                }
            }
            Err(e) => {
                println!("\x1b[32m{}, please improve current repo \x1b[34m{}\x1b[32m relevant configuration\x1b[0m", e, repo);
                env::setup_armory_configuration(&repo)?;
                let config = env::load_armory_configuration(&repo)?;
                token = common::get_user_token_of_armory(&repo, &config.username, &config.password).await?;
            }
        }
    }

    let current_dir = std::env::current_dir()?;
    let save_path = current_dir.to_str().unwrap();

    common::download_file_from_armory(&token, url, save_path, save_name).await?;

    Ok(())
}