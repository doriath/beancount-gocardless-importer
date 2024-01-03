use anyhow::Context;
use beanru::types::{
    Account, Amount, Currency, Directive, DirectiveContent, Ledger, MetadataValue, Posting,
    Transaction,
};
use clap::{Parser, Subcommand};
use gocardless::models::{
    JwtRefreshRequest, SpectacularJwtObtain, Status1c5Enum, TransactionSchema,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
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
        /// The ID of the account to list transactions for.
        /// The IDs can be seen through `list-requisitions` command.
        account_id: String,
    },
    Import {
        /// Import transactions based on configuration in given beancount ledger.
        ///
        /// The ledger is expected to have special metadata on the account that configures the
        /// importer. For more information, see README.md
        beancount_path: PathBuf,
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
    // TODO: update the file with the new token to avoid always refreshing it.
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

fn narration(t: &TransactionSchema) -> Option<String> {
    if let Some(inf) = &t.remittance_information_unstructured_array {
        if !inf.is_empty() {
            return Some(inf.join(", "));
        }
    }
    if let Some(inf) = &t.remittance_information_unstructured {
        return Some(inf.clone());
    }
    t.creditor_name.clone()
}

fn gocardless_transaction_to_beancount(
    t: &TransactionSchema,
    account: &Account,
) -> anyhow::Result<Directive<Decimal>> {
    let (date, _) = chrono::NaiveDate::parse_and_remainder(
        t.booking_date.as_ref().context("booking date is missing")?,
        "%Y-%m-%d",
    )?;
    let mut metadata: HashMap<String, MetadataValue<Decimal>> = HashMap::new();
    if let Some(dt) = &t.booking_date_time {
        metadata.insert(
            "booking_date_time".into(),
            MetadataValue::String(dt.clone()),
        );
    }
    if let Some(dt) = &t.value_date_time {
        metadata.insert("value_date_time".into(), MetadataValue::String(dt.clone()));
    }
    if let Some(debtor_name) = &t.debtor_name {
        metadata.insert(
            "from_name".into(),
            MetadataValue::String(debtor_name.clone()),
        );
    }
    if let Some(d) = &t.debtor_account {
        if let Some(iban) = &d.iban {
            metadata.insert("from_iban".into(), MetadataValue::String(iban.clone()));
        }
    }
    if let Some(creditor_name) = &t.creditor_name {
        metadata.insert(
            "to_name".into(),
            MetadataValue::String(creditor_name.clone()),
        );
    }
    if let Some(d) = &t.creditor_account {
        if let Some(iban) = &d.iban {
            metadata.insert("to_iban".into(), MetadataValue::String(iban.clone()));
        }
    }
    if let Some(ce) = &t.currency_exchange {
        if let Some(sc) = &ce.source_currency {
            metadata.insert("source_currency".into(), MetadataValue::String(sc.clone()));
        }
        if let Some(sc) = &ce.exchange_rate {
            metadata.insert("exchange_rate".into(), MetadataValue::String(sc.clone()));
        }
        if let Some(sc) = &ce.target_currency {
            metadata.insert("target_currency".into(), MetadataValue::String(sc.clone()));
        }
    }
    if let Some(tc) = &t.proprietary_bank_transaction_code {
        metadata.insert("transaction_code".into(), MetadataValue::String(tc.clone()));
    }

    let mut links = HashSet::new();
    if let Some(id) = &t.internal_transaction_id {
        links.insert(format!("id-{}", id));
    }

    let transaction = Transaction {
        flag: None,
        payee: None,
        narration: narration(t),
        tags: Default::default(),
        links,
        postings: vec![Posting {
            flag: None,
            account: account.clone(),
            amount: Some(Amount {
                value: t.transaction_amount.amount.parse()?,
                currency: Currency(t.transaction_amount.currency.clone()),
            }),
            cost: None,
            price: None,
            metadata: Default::default(),
            autocomputed: false,
        }],
        balanced: false,
    };
    let d = Directive {
        date,
        content: DirectiveContent::Transaction(transaction),
        metadata,
    };
    Ok(d)
}

fn is_duplicate(d: &Directive<Decimal>, ids: &HashSet<String>) -> bool {
    let Some(t) = d.content.transaction_opt() else { return false; };
    for link in &t.links {
        if ids.contains(link) {
            return true;
        }
    }
    false
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
        Commands::Import { beancount_path } => {
            let config = config_with_token().await?;

            let mut ledger: Ledger<Decimal> = Ledger::read(beancount_path, |p| async {
                Ok(tokio::fs::read_to_string(p).await?)
            })
            .await?;

            let mut ids: HashSet<String> = HashSet::new();
            for (_, file) in &mut ledger.files {
                for d in &file.directives {
                    if let DirectiveContent::Transaction(t) = &d.content {
                        for link in &t.links {
                            if link.starts_with("id-") {
                                ids.insert(link.clone());
                            }
                        }
                    }
                }
            }

            for (_, file) in &mut ledger.files {
                let mut to_import: Vec<(String, Account)> = vec![];
                for d in &file.directives {
                    if let DirectiveContent::Open(ref open) = d.content {
                        let Some(importer)  = d.metadata.get("importer") else { continue };
                        let MetadataValue::String(importer) = importer else { continue };
                        if importer != "gocardless" {
                            continue;
                        }

                        let Some(account_id)  = d.metadata.get("account_id") else { continue };
                        let MetadataValue::String(account_id) = account_id else { continue };
                        to_import.push((account_id.clone(), open.account.clone()));
                    }
                }
                for (account_id, account) in to_import {
                    println!("Retrieving transactions for {} ...", account);
                    let res = gocardless::apis::accounts_api::retrieve_account_transactions(
                        &config,
                        &account_id,
                        None,
                        None,
                    )
                    .await?;

                    let mut new_directives = Vec::new();
                    for t in res.transactions.booked {
                        let d = gocardless_transaction_to_beancount(&t, &account)?;
                        if !is_duplicate(&d, &ids) {
                            new_directives.push(d);
                        }
                    }
                    new_directives.reverse();
                    new_directives.sort_by_key(|d| d.date);
                    file.directives.append(&mut new_directives);
                }
            }

            ledger
                .write(|p, content| async { Ok(tokio::fs::write(p, content).await?) })
                .await?;
        }
    }
    Ok(())
}
