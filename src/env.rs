use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum ConfigError {
    IoError(std::io::Error),
    JsonError(serde_json::Error),
    NotFound(String),
    Other(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConfigError::IoError(e) => write!(f, "IO error: {}", e),
            ConfigError::JsonError(e) => write!(f, "JSON error: {}", e),
            ConfigError::NotFound(msg) => write!(f, "Not found: {}", msg),
            ConfigError::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        ConfigError::IoError(err)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(err: serde_json::Error) -> Self {
        ConfigError::JsonError(err)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RepositoryConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ConfigFile {
    repositories: Vec<RepositoryConfig>,
}

// pub fn check_amr_config() -> Result<bool, ConfigError> {
//     let config_file = get_config_path()?;
//     Ok(config_file.exists())
// }

fn get_config_path() -> Result<PathBuf, ConfigError> {
    let home_dir = dirs::home_dir().ok_or_else(|| ConfigError::Other("Failed to get home directory".to_string()))?;
    Ok(home_dir.join(".amr").join("config.json"))
}

fn prompt_for_repository_config(url: &str) -> Result<RepositoryConfig, ConfigError> {
    print!("Enter username: ");
    io::stdout().flush()?;
    let mut username = String::new();
    io::stdin().read_line(&mut username)?;

    print!("Enter password: ");
    io::stdout().flush()?;
    let mut password = String::new();
    io::stdin().read_line(&mut password)?;

    Ok(RepositoryConfig {
        url: url.trim().to_string(),
        username: username.trim().to_string(),
        password: password.trim().to_string(),
    })
}

fn save_config(new_config: &RepositoryConfig) -> Result<(), ConfigError> {
    let config_file = get_config_path()?;
    let config_dir = config_file.parent().ok_or_else(|| ConfigError::Other("Invalid config path".to_string()))?;

    fs::create_dir_all(config_dir)?;

    let mut config_data = if config_file.exists() {
        let content = fs::read_to_string(&config_file)?;
        serde_json::from_str::<ConfigFile>(&content)?
    } else {
        ConfigFile { repositories: Vec::new() }
    };

    let mut found = false;
    for repo in &mut config_data.repositories {
        if repo.url == new_config.url {
            *repo = new_config.clone();
            found = true;
            break;
        }
    }

    if !found {
        config_data.repositories.push(new_config.clone());
    }

    let content = serde_json::to_string_pretty(&config_data)?;
    fs::write(&config_file, content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&config_file)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&config_file, perms)?;
    }

    Ok(())
}

pub fn setup_armory_configuration(url: &str) -> Result<(), ConfigError> {
    let config = prompt_for_repository_config(url)?;
    save_config(&config)?;
    println!("Configuration saved successfully to ~/.amr/config.json");
    Ok(())
}

pub fn load_armory_configuration(target_url: &str) -> Result<RepositoryConfig, ConfigError> {
    let config_file = get_config_path()?;

    if !config_file.exists() {
        return Err(ConfigError::NotFound(format!("Config file does not exist at {}", config_file.display())));
    }

    let content = fs::read_to_string(&config_file)?;
    let config_data: ConfigFile = serde_json::from_str(&content)?;

    for repo in config_data.repositories {
        if repo.url == target_url {
            return Ok(repo);
        }
    }

    Err(ConfigError::NotFound(format!("No configuration found for URL: {}", target_url)))
}