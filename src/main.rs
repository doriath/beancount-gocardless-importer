use clap::{Parser, Subcommand};
use gocardless::models::SpectacularJwtObtain;
use serde::{Deserialize, Serialize};
use std::{os::unix::fs::PermissionsExt, fs::OpenOptions};
use tokio::io::AsyncWriteExt;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    SignIn {
        #[arg(long)]
        secret_id: String,
        #[arg(long)]
        secret_key: String,
    },
}

#[derive(Serialize, Deserialize, Debug)]
struct Token {
    /// Time when jwt was obtained.
    time: std::time::SystemTime,
    /// The jwt (access & refresh tokens).
    jwt: SpectacularJwtObtain,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::SignIn { secret_id, secret_key } => {
            let config = gocardless::apis::configuration::Configuration::default();
            let secrets = gocardless::models::jwt_obtain_pair_request::JwtObtainPairRequest::new(
                secret_id, secret_key);
            let time = std::time::SystemTime::now();
            let jwt = gocardless::apis::token_api::obtain_new_access_slash_refresh_token_pair(&config, secrets).await?;
            let token = Token { time, jwt };

            let token_yaml_dir = std::path::PathBuf::from(std::env!("HOME")).join(".gocardless");
            tokio::fs::create_dir_all(&token_yaml_dir).await?;
            let dir_permissions = std::fs::Permissions::from_mode(0o700);
            tokio::fs::set_permissions(&token_yaml_dir, dir_permissions).await?;

            let token_yaml_path = token_yaml_dir.join("token.yml");
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create(true).mode(0o600);
            let mut file = options.open(token_yaml_path).await?;
            file.write_all(serde_yaml::to_string(&token)?.as_bytes()).await?;
            println!("Signed in");
        }
    }
    Ok(())
}
