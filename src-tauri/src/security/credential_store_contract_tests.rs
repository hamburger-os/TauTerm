use super::credential_store::{CredentialStore, CredentialType, CredentialValue};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const MASTER_PASSWORD: &str = "TauTerm v0.5.2 credential contract password";

fn unique_account(suffix: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after UNIX epoch")
        .as_nanos();
    format!("tauterm-v052-{suffix}-{}-{nonce}", std::process::id())
}

fn unlock_fallback_if_needed(store: &CredentialStore) {
    if !store.status().native_available {
        store
            .unlock_fallback(MASTER_PASSWORD)
            .expect("fallback vault should unlock");
    }
}

fn assert_password(value: CredentialValue, expected: &str) {
    match value {
        CredentialValue::Password(actual) => assert_eq!(actual, expected),
        other => panic!("expected password credential, got {other:?}"),
    }
}

#[test]
fn credential_store_persists_round_trip_and_delete() {
    let dir = tempfile::tempdir().expect("temporary credential directory");
    let account = unique_account("roundtrip");
    let secret = "correct horse battery staple";

    let store = CredentialStore::with_data_dir(dir.path().to_path_buf());
    let initial_status = store.status();

    // GitHub's Ubuntu runner intentionally has no desktop Secret Service session.
    // Keep this as an explicit release-contract assertion so the fallback path
    // cannot silently disappear from the cross-platform CI matrix.
    #[cfg(target_os = "linux")]
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        assert!(
            !initial_status.native_available,
            "Ubuntu CI must exercise the authenticated encrypted fallback"
        );
    }

    unlock_fallback_if_needed(&store);
    store
        .store_credential(
            &account,
            CredentialType::Password,
            CredentialValue::Password(secret.to_owned()),
            "v0.5.2 contract test",
        )
        .expect("credential store should accept a password");

    assert_password(
        store
            .get_credential(&account)
            .expect("stored credential should be readable"),
        secret,
    );
    assert!(
        store
            .list_credentials()
            .expect("credential list should be readable")
            .iter()
            .any(|entry| entry.account == account),
        "stored credential must appear in the index"
    );

    // Re-open the store to prove persistence rather than merely in-memory state.
    drop(store);
    let reopened = CredentialStore::with_data_dir(dir.path().to_path_buf());
    if !initial_status.native_available {
        assert!(
            reopened.get_credential(&account).is_err(),
            "fallback vault must stay locked across store instances"
        );
        reopened
            .unlock_fallback(MASTER_PASSWORD)
            .expect("fallback vault should unlock after reopen");
    }

    assert_password(
        reopened
            .get_credential(&account)
            .expect("persisted credential should survive reopen"),
        secret,
    );
    reopened
        .delete_credential(&account)
        .expect("credential delete should succeed");
    assert!(
        reopened.get_credential(&account).is_err(),
        "deleted credential must no longer be readable"
    );
}

#[test]
fn fallback_vault_has_explicit_version_and_rejects_unknown_versions() {
    let dir = tempfile::tempdir().expect("temporary credential directory");
    let store = CredentialStore::with_data_dir(dir.path().to_path_buf());

    // Native keyring platforms exercise their backend in the round-trip test.
    // The headless Linux CI leg exercises this fallback-specific contract.
    if store.status().native_available {
        return;
    }

    store
        .unlock_fallback(MASTER_PASSWORD)
        .expect("fallback vault should initialize");
    store
        .store_credential(
            &unique_account("version"),
            CredentialType::Token,
            CredentialValue::Token("token-value".to_owned()),
            "v0.5.2 version contract",
        )
        .expect("fallback vault should persist a credential");
    store.lock_fallback();

    let vault_path = dir.path().join("credentials.vault.json");
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&vault_path).expect("fallback vault file should exist"))
            .expect("fallback vault envelope should be JSON");
    assert_eq!(
        envelope.get("version").and_then(serde_json::Value::as_u64),
        Some(1),
        "v1 is TauTerm's first persisted credential-vault format"
    );

    envelope["version"] = serde_json::Value::from(999_u64);
    fs::write(
        &vault_path,
        serde_json::to_vec_pretty(&envelope).expect("serialize modified envelope"),
    )
    .expect("write modified envelope");

    let reopened = CredentialStore::with_data_dir(dir.path().to_path_buf());
    assert!(
        reopened.unlock_fallback(MASTER_PASSWORD).is_err(),
        "unknown vault versions must fail closed instead of being guessed or migrated unsafely"
    );
}
