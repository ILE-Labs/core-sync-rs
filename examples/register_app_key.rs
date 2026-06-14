//! Run this once to register the app with your local indexd and get a working AppKey.
//!
//! ```bash
//! cargo run --example register_app_key --features sia-sdk
//! ```
//!
//! It will print an approval URL. Open it in your browser while indexd is running,
//! approve the request, then come back here and it will print the 64-hex AppKey.
//! Paste that value into `.env` as `SIA_APP_KEY`.
//!
//! Required environment:
//! - `SIA_INDEXER_URL` (defaults to `http://localhost:9982`)
//! - `SIA_RECOVERY_PHRASE`

#[cfg(feature = "sia-sdk")]
#[tokio::main]
async fn main() {
    use sia_storage::{AppMetadata, Builder, app_id};

    const APP_META: AppMetadata = AppMetadata {
        id: app_id!("c0e5790c5e796e63000000000000000000000000000000000000000000000001"),
        name: "core-sync-rs",
        description: "Local differential sync for Sia",
        service_url: "https://github.com/ile-labs/core-sync-rs",
        logo_url: None,
        callback_url: None,
    };

    let indexer_url = std::env::var("SIA_INDEXER_URL")
        .unwrap_or_else(|_| "http://localhost:9982".to_string());

    println!("Connecting to indexd at {indexer_url}...");

    let builder = Builder::new(&indexer_url, APP_META).expect("failed to create builder");

    let requesting = builder
        .request_connection()
        .await
        .expect("failed to request connection");

    println!("\n========================================");
    println!("OPEN THIS URL IN YOUR BROWSER TO APPROVE:");
    println!("{}", requesting.response_url());
    println!("========================================\n");
    println!("Waiting for you to approve in the browser (up to 5 minutes)...");

    let approved = requesting
        .wait_for_approval()
        .await
        .expect("approval failed or expired");

    println!("Approved! Now registering with wallet mnemonic...");

    let mnemonic = std::env::var("SIA_RECOVERY_PHRASE")
        .expect("set SIA_RECOVERY_PHRASE before registering an app key");

    let sdk = approved
        .register(&mnemonic)
        .await
        .expect("registration failed");

    let app_key = sdk.app_key();
    let exported = app_key.export();
    let hex_key = hex::encode(exported);

    println!("\n========================================");
    println!("SUCCESS! Your AppKey (save this in .env):");
    println!("SIA_APP_KEY={hex_key}");
    println!("========================================\n");
    println!("Now run the live demo:");
    println!("  export SIA_INDEXER_URL={indexer_url}");
    println!("  export SIA_APP_KEY={hex_key}");
    println!("  cargo run --example sia_live_demo --features sia-sdk -- ./testfile.txt");
}

#[cfg(not(feature = "sia-sdk"))]
fn main() {
    eprintln!("Run with: cargo run --example register_app_key --features sia-sdk");
}
