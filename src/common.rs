use reqwest::header::{CONTENT_DISPOSITION, HeaderMap};
use reqwest::Client;
use std::error::Error;
use std::fmt;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use serde::{Deserialize, Serialize};
use futures_util::StreamExt;
use std::borrow::Cow;
use indicatif::{ProgressBar, ProgressStyle, ProgressDrawTarget};
use terminal_size::{terminal_size, Width};

#[derive(Debug)]
pub enum DownloadError {
    ReqwestError(reqwest::Error),
    IoError(std::io::Error),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DownloadError::ReqwestError(e) => write!(f, "Reqwest error: {}", e),
            DownloadError::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl Error for DownloadError {}

impl From<reqwest::Error> for DownloadError {
    fn from(err: reqwest::Error) -> Self {
        DownloadError::ReqwestError(err)
    }
}

impl From<std::io::Error> for DownloadError {
    fn from(err: std::io::Error) -> Self {
        DownloadError::IoError(err)
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct LoginResponse {
    status: i32,
    message: String,
    field_errors: Option<serde_json::Value>,
    data: LoginData,
}

#[derive(Serialize, Deserialize, Debug)]
struct LoginData {
    id: i32,
    username: String,
    jti: String,
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
}

pub fn parse_repo_url(full_url: &str) -> Result<String, Box<dyn Error>> {
    if !full_url.contains("armory") {
        return Err("Not armory URL".into());
    }
    
    let url = reqwest::Url::parse(full_url)?;
    let base_url = format!("{}://{}", url.scheme(), url.host().ok_or("Invalid URL")?);
    Ok(base_url)
}

fn get_file_name_from_headers(headers: &HeaderMap) -> Option<String> {
    let content_disposition = headers.get(CONTENT_DISPOSITION)?.to_str().ok()?;

    content_disposition
        .split("filename*=UTF-8''")
        .nth(1)
        .map(Cow::from)
        .or_else(|| {
            content_disposition
                .split("filename=")
                .nth(1)
                .map(|s| {
                    // 处理引号和分号
                    let s = s.trim();
                    let s = s.trim_matches('"');
                    let s = s.split(';').next().unwrap_or(s);
                    Cow::from(s.trim())
                })
        })
        .map(|s| s.into_owned())
}

fn get_file_name_from_url(url: &str) -> String {
    Path::new(url)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download")
        .to_string()
}

pub async fn get_user_token_of_armory(
    url: &str,
    username: &str,
    password: &str,
) -> Result<String, Box<dyn Error>> {
    let client = Client::new();
    let login_url = format!("{}/usercenter/v1/auth/login", url);
    
    let data = serde_json::json!({
        "account": username,
        "password": password
    });

    println!("Attempting login to: {}", login_url);
    println!("Using credentials - username: {}", username);

    let response = client
        .post(&login_url)
        .json(&data)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await?;
        return Err(format!("Login failed with status {}: {}", status, body).into());
    }

    let raw_response = response.text().await?;
    // println!("Raw login response: {}", raw_response);

    let login_response: LoginResponse = serde_json::from_str(&raw_response)
        .map_err(|e| format!("Failed to parse login response: {}\nRaw response: {}", e, raw_response))?;

    if login_response.data.access_token.is_empty() {
        return Err("Server returned empty access token".into());
    }

    println!("Successfully obtained token from {}", url);
    Ok(login_response.data.access_token)
}

pub async fn download_file_from_armory(
    token: &str,
    src_url: &str,
    save_path: &str,
    save_name: Option<&str>,
) -> Result<String, Box<dyn Error>> {
    let client = Client::new();
    let path = Path::new(save_path);
    
    if !path.exists() {
        fs::create_dir_all(path).await?;
    }

    let file_name = match save_name {
        Some(name) => {
            let name = name.to_string();
            println!("Using specified filename: {}", name);
            name
        },
        None => {
            let response = Client::new()
                .get(src_url)
                .header("Cookie", format!("USER_TOKEN={}", token))
                .send()
                .await?;

            let filename = get_file_name_from_headers(response.headers())
                .unwrap_or_else(|| {
                    let url_name = get_file_name_from_url(src_url);
                    println!("Falling back to URL filename: {}", url_name);
                    url_name
                });

            println!("filename: {}", filename);
            filename
        }
    };


    let final_path = path.join(&file_name);
    let temp_path = path.join(format!("{}.part", &file_name));

    let mut start_byte = 0;
    if temp_path.exists() {
        let metadata = fs::metadata(&temp_path).await?;
        start_byte = metadata.len();
        println!("Resuming download from byte: {}", start_byte);
    }

    let pb = ProgressBar::hidden();
    let terminal_width = terminal_size()
    .map(|(Width(w), _)| w as usize)
    .unwrap_or(80);
    let _bar_width = (terminal_width.saturating_sub(45))
    .clamp(10, terminal_width.saturating_sub(45));

    pb.set_style(ProgressStyle::default_bar()
        .template(&format!(
            "{{spinner:.green}} {{elapsed_precise}} [{{bar:{}.cyan/blue}}] {{bytes}} / {{total_bytes}} ({{eta}})",
            _bar_width
        ))
        .progress_chars("=>-"));

    let mut request = client
        .get(src_url)
        .header("Cookie", format!("USER_TOKEN={}", token));

    if start_byte > 0 {
        request = request.header("Range", format!("bytes={}-", start_byte));
    }

    let response = request.send().await?;

    let total_size = if start_byte > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT {

        response.headers()
            .get("Content-Range")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.split('/').last())
            .and_then(|s| s.parse().ok())
            .unwrap_or(start_byte + response.content_length().unwrap_or(0))
    } else {
        response.content_length().unwrap_or(0)
    };


    pb.set_length(total_size);
    pb.set_position(start_byte);
    pb.reset_eta();
    pb.println(format!("Starting download: {}", file_name));

    pb.set_draw_target(ProgressDrawTarget::stdout());

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&temp_path)
        .await?;

    let mut stream = response.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        file.write_all(&chunk).await?;
        pb.inc(chunk.len() as u64);
    }

    pb.finish_with_message(format!("Downloaded {}", file_name));
    fs::rename(&temp_path, &final_path).await?;

    Ok(file_name)
}
