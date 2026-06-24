# security-model

## Purpose

定义 TauTerm 的安全模型，包括主机密钥验证、TLS 证书固定、代理转发控制和日志脱敏机制。

## ADDED Requirements

### Requirement: SSH host key verification
The system SHALL verify SSH host keys against a local `known_hosts` store before establishing SSH connections.
On first connection to an unknown host, the system SHALL prompt the user to confirm the host key fingerprint.
Host key mismatches SHALL abort the connection with a clear warning message.
The `known_hosts` store SHALL support manual entry, editing, and removal of host entries.

#### Scenario: First connection to a new SSH host
- **WHEN** the user connects to an SSH host for the first time
- **THEN** the system SHALL display the host key fingerprint and ask the user to confirm before proceeding

#### Scenario: Host key mismatch detected
- **WHEN** a previously known host presents a different host key
- **THEN** the system SHALL abort the connection and display a security warning about potential MITM attack

### Requirement: TLS certificate pinning for encrypted protocols
The system SHALL support TLS certificate pinning for protocols that use TLS (TRDP, Telnet TLS).
Pinned certificates SHALL be stored alongside the credential in the Credential Store.
Certificate verification failures SHALL present the user with the certificate details and require explicit acceptance to proceed.

#### Scenario: TRDP connection with pinned certificate
- **WHEN** connecting to a TRDP endpoint with a pinned TLS certificate
- **THEN** the system SHALL verify the server's certificate against the pinned certificate and reject if mismatched

### Requirement: SSH agent forwarding control
The system SHALL support SSH agent forwarding with an explicit opt-in per session.
Agent forwarding SHALL be disabled by default.
The SSH plugin SHALL display a warning when agent forwarding is enabled.

#### Scenario: User enables agent forwarding
- **WHEN** the user checks "Enable SSH Agent Forwarding" in the SSH session config
- **THEN** the system SHALL display a security warning and require confirmation before enabling

### Requirement: Log sanitization for sensitive data
The system SHALL automatically sanitize log output to remove passwords, private keys, tokens, and other credential material.
Sanitization SHALL replace sensitive content with `[REDACTED]` before writing to log files or console output.
The sanitizer SHALL use pattern matching for common credential formats (password fields, `-----BEGIN` key markers, `Authorization: Bearer` headers).

#### Scenario: Password appears in a debug log
- **WHEN** a debug-level log message contains a password parameter
- **THEN** the log sanitizer SHALL replace the password value with `[REDACTED]` before the log is written

#### Scenario: SSH private key does not leak to logs
- **WHEN** an error occurs during SSH key loading
- **THEN** the error log SHALL NOT contain the key material, only the key filename and error type

### Requirement: Permission prompts for sensitive operations
The system SHALL require user confirmation via a permission prompt for: credential export, port forwarding creation, and file system access outside configured directories.
Bulk operations (e.g., "delete all credentials") SHALL require double confirmation.
Permission prompts SHALL clearly describe what is being accessed or modified.

#### Scenario: User exports a stored SSH key
- **WHEN** the user initiates export of an SSH private key
- **THEN** the system SHALL display a confirmation dialog describing the security implications before proceeding
