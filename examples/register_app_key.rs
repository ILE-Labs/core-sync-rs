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
//! - `SIA_INDEXER_URL` (defaults to `http://127.0.0.1:9982`)
//!   NOTE: Use `127.0.0.1`, not `localhost`. indexd verifies request signatures
//!   against the literal IP it binds to; `localhost` causes a hash mismatch and
//!   an "invalid signature" error.
//! - `SIA_RECOVERY_PHRASE` — your indexd wallet mnemonic

#[cfg(feature = "sia-sdk")]
#[tokio::main]
async fn main() {
    // Re-use the single shared constants — no need to duplicate env var names or
    // APP_META here; they live in sia_sdk.rs and are the authoritative definitions.
    use core_sync_rs::sia_sdk::{APP_META, SIA_APP_KEY_ENV, SIA_INDEXER_URL_ENV};
    use sia_storage::Builder;

    let indexer_url = std::env::var(SIA_INDEXER_URL_ENV)
        .unwrap_or_else(|_| "http://127.0.0.1:9982".to_string());

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
    let hex_key = hex::encode(app_key.export());

    println!("\n========================================");
    println!("SUCCESS! Your AppKey (save this in .env):");
    println!("{SIA_APP_KEY_ENV}={hex_key}");
    println!("========================================\n");
    println!("Now run the live demo:");
    println!("  export {SIA_INDEXER_URL_ENV}={indexer_url}");
    println!("  export {SIA_APP_KEY_ENV}={hex_key}");
    println!("  cargo run --example sia_live_demo --features sia-sdk -- ./testfile.txt");
}

#[cfg(not(feature = "sia-sdk"))]
fn main() {
    eprintln!("Run with: cargo run --example register_app_key --features sia-sdk");
}
