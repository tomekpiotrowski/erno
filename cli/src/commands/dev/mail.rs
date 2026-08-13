use std::collections::HashSet;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;

use super::{MAGENTA, RESET};

#[derive(Debug, Deserialize)]
struct MockEmail {
    id: String,
    to: String,
    subject: String,
}

pub fn spawn_mail_watcher(api_url: String) {
    tokio::spawn(async move {
        let client = match Client::builder().timeout(Duration::from_secs(2)).build() {
            Ok(c) => c,
            Err(_) => return,
        };
        let url = format!("{}/dev/emails", api_url.trim_end_matches('/'));
        let mut seen = HashSet::new();
        // Wait for the API to come up before treating the first snapshot as "new".
        let mut primed = false;
        loop {
            if let Ok(list) = fetch_emails(&client, &url).await {
                if !primed {
                    for email in &list {
                        seen.insert(email.id.clone());
                    }
                    primed = true;
                } else {
                    for email in list {
                        if seen.insert(email.id.clone()) {
                            println!("{MAGENTA}[mail]{RESET} {} → {}", email.subject, email.to);
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

async fn fetch_emails(client: &Client, url: &str) -> Result<Vec<MockEmail>, ()> {
    let res = client.get(url).send().await.map_err(|_| ())?;
    if !res.status().is_success() {
        return Err(());
    }
    res.json().await.map_err(|_| ())
}
