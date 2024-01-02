use anyhow::Context;
use clap::{Parser, Subcommand};
use gocardless::models::{JwtRefreshRequest, SpectacularJwtObtain, Status1c5Enum};
use serde::{Deserialize, Serialize};
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, SystemTime},
};
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
        secret_id: String,
        secret_key: String,
    },
    ListInstitutions {
        #[arg(long)]
        country: Option<String>,
    },
    CreateRequisition {
        institution_id: String,
    },
    ListRequisitions,
    DeleteRequisition {
        requisition_id: String,
    },
    ListTransactions {
        account_id: String,
    },
}

#[derive(Serialize, Deserialize, Debug)]
struct Tokens {
    access_token: String,
    access_expires: SystemTime,
    refresh_token: String,
    refresh_expires: SystemTime,
}

impl Tokens {
    fn from_jwt(now: SystemTime, jwt: &SpectacularJwtObtain) -> anyhow::Result<Tokens> {
        Ok(Tokens {
            access_token: jwt.access.clone().context("access token is missing")?,
            access_expires: now
                + Duration::from_secs(
                    jwt.access_expires
                        .context("access token expiration is missing")?
                        .try_into()?,
                ),
            refresh_token: jwt.refresh.clone().context("refresh token is missing")?,
            refresh_expires: now
                + Duration::from_secs(
                    jwt.refresh_expires
                        .context("refresh token expiration is missing")?
                        .try_into()?,
                ),
        })
    }
}

async fn get_token() -> anyhow::Result<String> {
    let path = std::path::PathBuf::from(std::env!("HOME"))
        .join(".gocardless")
        .join("token.yml");
    let tokens: Tokens = serde_yaml::from_str(&tokio::fs::read_to_string(path).await?)?;
    if SystemTime::now() < tokens.access_expires {
        return Ok(tokens.access_token);
    }
    if SystemTime::now() > tokens.refresh_expires {
        anyhow::bail!("refresh token exipred")
    }
    let config = gocardless::apis::configuration::Configuration::default();
    let jwt = gocardless::apis::token_api::get_a_new_access_token(
        &config,
        JwtRefreshRequest::new(tokens.refresh_token),
    )
    .await?;
    // TODO: update the file
    Ok(jwt.access.unwrap())
}

async fn config_with_token() -> anyhow::Result<gocardless::apis::configuration::Configuration> {
    let token = get_token()
        .await
        .context("Failed to get the access token, please first run `sign-in` command")?;
    Ok(gocardless::apis::configuration::Configuration {
        bearer_access_token: Some(token),
        ..Default::default()
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::SignIn {
            secret_id,
            secret_key,
        } => {
            let config = gocardless::apis::configuration::Configuration::default();
            let secrets = gocardless::models::jwt_obtain_pair_request::JwtObtainPairRequest::new(
                secret_id, secret_key,
            );
            let jwt = gocardless::apis::token_api::obtain_new_access_slash_refresh_token_pair(
                &config, secrets,
            )
            .await?;

            let tokens = Tokens::from_jwt(SystemTime::now(), &jwt)?;

            let token_yaml_dir = std::path::PathBuf::from(std::env!("HOME")).join(".gocardless");
            tokio::fs::create_dir_all(&token_yaml_dir).await?;
            let dir_permissions = std::fs::Permissions::from_mode(0o700);
            tokio::fs::set_permissions(&token_yaml_dir, dir_permissions).await?;

            let token_yaml_path = token_yaml_dir.join("token.yml");
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create(true).mode(0o600);
            let mut file = options.open(token_yaml_path).await?;
            file.write_all(serde_yaml::to_string(&tokens)?.as_bytes())
                .await?;
            println!("Signed in");
        }
        Commands::ListInstitutions { country } => {
            let config = config_with_token().await?;
            let banks = gocardless::apis::institutions_api::retrieve_all_supported_institutions_in_a_given_country(&config, None, None, None, None, None, country.as_deref(), None, None, None, None, None, None, None).await?;
            println!("ID: NAME");
            for bank in banks {
                println!("{}: {}", bank.id, bank.name);
            }
        }
        Commands::CreateRequisition { institution_id } => {
            let config = config_with_token().await?;
            let req = gocardless::models::RequisitionRequest::new(
                Some("https://example.com/".into()),
                institution_id,
            );
            let res = gocardless::apis::requisitions_api::create_requisition(&config, req).await?;
            let link = res
                .link
                .context("setup link is missing from the gocardless response")?;
            println!("Follow the link to finish the instituion setup:\n{}", link);
        }
        Commands::ListRequisitions => {
            let config = config_with_token().await?;
            let res =
                gocardless::apis::requisitions_api::retrieve_all_requisitions(&config, None, None)
                    .await?;
            let Some(requisitions) = res.results else { return Ok(()) };
            for r in requisitions {
                println!("ID: {:?}", r.id);
                println!("Institution ID: {}", r.institution_id);
                println!("Agreement: {:?}", r.agreement);
                match r.status {
                    Some(Status1c5Enum::Cr) => println!("Status: Created (not set up yet)"),
                    Some(Status1c5Enum::Ln) => println!("Status: Linked"),
                    _ => println!("Status: {:?}", r.status),
                }
                println!("Link: {:?}", r.link);
                if let Some(ref accounts) = r.accounts {
                    println!("Accounts:");
                    for a in accounts {
                        println!("- {}", a);
                    }
                }
                println!();
            }
        }
        Commands::DeleteRequisition { requisition_id } => {
            let config = config_with_token().await?;
            gocardless::apis::requisitions_api::delete_requisition_by_id(&config, &requisition_id)
                .await?;
        }
        Commands::ListTransactions { account_id } => {
            let config = config_with_token().await?;
            let res = gocardless::apis::accounts_api::retrieve_account_transactions(
                &config,
                &account_id,
                None,
                None,
            )
            .await?;
            println!("{}", serde_yaml::to_string(&res)?);
        }
    }
    Ok(())
}
