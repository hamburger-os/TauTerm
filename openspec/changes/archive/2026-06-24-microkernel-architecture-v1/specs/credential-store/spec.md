# credential-store

## Purpose

定义 TauTerm 的加密凭据存储系统，安全地管理密码、SSH 密钥、证书和 Token 等敏感信息。

## ADDED Requirements

### Requirement: Credential Store uses OS keyring as primary backend
The system SHALL use `keyring-rs` to store credentials in the operating system's secure credential manager (macOS Keychain, Windows Credential Manager, Linux Secret Service).
The system SHALL support four credential types: `password`, `ssh_key`, `certificate`, and `token`.
Each credential entry SHALL be identified by a unique `(service, account)` tuple where `service = "tauterm"` and `account` is a user-defined label.

#### Scenario: Store an SSH private key
- **WHEN** the user saves an SSH private key for "production-server"
- **THEN** the credential SHALL be stored in the OS keyring under service "tauterm" and account "production-server" with type "ssh_key"

#### Scenario: Retrieve a stored password
- **WHEN** the SSH plugin requests the password for "staging-server"
- **THEN** the Credential Store SHALL retrieve it from the OS keyring and return it as a `Secret<String>`

### Requirement: AES-256-GCM fallback for systems without keyring
The system SHALL fall back to AES-256-GCM encrypted file storage when the OS keyring is unavailable.
The encryption key SHALL be derived from a machine-specific identifier combined with a randomly generated salt stored alongside the encrypted data.
The fallback file SHALL be stored in the Tauri app data directory with restricted file permissions (0600 on Unix).

#### Scenario: Keyring unavailable on headless Linux
- **WHEN** the Credential Store initializes and `keyring-rs` reports Secret Service unavailable
- **THEN** the system SHALL log a warning and use AES-256-GCM encrypted file storage

#### Scenario: Fallback file is tampered with
- **WHEN** the AES-256-GCM decryption fails due to file modification
- **THEN** the system SHALL return an authentication error and NOT crash or leak partial data

### Requirement: Credential types are type-safe
The system SHALL use a typed `Credential` enum distinguishing between `Password(String)`, `SshKey { private_key: String, passphrase: Option<String> }`, `Certificate { cert_data: Vec<u8>, key_data: Vec<u8> }`, and `Token(String)`.
The system SHALL NOT allow implicit conversion between credential types.

#### Scenario: Password credential cannot be used as SSH key
- **WHEN** the SSH plugin requests an `SshKey` credential but the stored entry is a `Password`
- **THEN** the Credential Store SHALL return a type mismatch error

### Requirement: Credential deletion is immediate and unrecoverable
The system SHALL support immediate deletion of stored credentials.
Deleted credentials SHALL be removed from both the keyring and the fallback file.
Deleted credentials SHALL NOT be recoverable.

#### Scenario: User deletes a saved password
- **WHEN** the user initiates deletion of credential "old-server"
- **THEN** the entry SHALL be removed from the keyring and the fallback file, and subsequent retrieval SHALL return NotFound

### Requirement: Credential Store exposes Tauri commands with audit logging
The system SHALL expose `store_credential`, `get_credential`, `list_credentials`, and `delete_credential` Tauri commands.
All credential access SHALL be logged (type and account, but NOT the secret value) for audit purposes.
The `get_credential` command SHALL require explicit user confirmation for sensitive operations.

#### Scenario: Frontend lists available credentials
- **WHEN** the frontend calls `list_credentials()`
- **THEN** the system SHALL return a list of `(account, type)` tuples without exposing secret values
