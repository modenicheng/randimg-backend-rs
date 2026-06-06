//! Unit tests for auth module: JWT token creation/verification and password hashing.

#![cfg(feature = "http")]

use randimg_core::auth::jwt::{Claims, create_token, verify_token};
use randimg_core::auth::password::{hash_password, verify_password};

// ── JWT Tests ────────────────────────────────────────────────────────────────

#[test]
fn test_create_and_verify_token_roundtrip() {
    let secret = "test-secret-key-123";
    let token = create_token("alice", secret, 60).unwrap();
    let claims = verify_token(&token, secret).expect("token should be valid");
    assert_eq!(claims.sub, "alice");
}

#[test]
fn test_verify_token_wrong_secret_fails() {
    let token = create_token("alice", "correct-secret", 60).unwrap();
    let result = verify_token(&token, "wrong-secret");
    assert!(result.is_err());
}

#[test]
fn test_verify_token_expired() {
    // Create a token that expired 1 hour ago by crafting claims directly
    use jsonwebtoken::{EncodingKey, Header, encode};
    let claims = Claims {
        sub: "alice".to_string(),
        exp: (chrono::Utc::now() - chrono::Duration::hours(1)).timestamp() as usize,
        iss: "randimg".to_string(),
        aud: "randimg-api".to_string(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret("secret".as_bytes()),
    )
    .unwrap();
    let result = verify_token(&token, "secret");
    assert!(result.is_err());
}

#[test]
fn test_create_token_different_users_different_tokens() {
    let secret = "test-secret";
    let token_a = create_token("alice", secret, 60).unwrap();
    let token_b = create_token("bob", secret, 60).unwrap();
    assert_ne!(token_a, token_b);

    let claims_a = verify_token(&token_a, secret).unwrap();
    let claims_b = verify_token(&token_b, secret).unwrap();
    assert_eq!(claims_a.sub, "alice");
    assert_eq!(claims_b.sub, "bob");
}

#[test]
fn test_create_token_contains_expected_claims() {
    let secret = "test-secret";
    let token = create_token("carol", secret, 30).unwrap();
    let claims = verify_token(&token, secret).unwrap();
    assert_eq!(claims.sub, "carol");
    // exp should be roughly now + 30 minutes
    let now = chrono::Utc::now().timestamp() as usize;
    assert!(claims.exp > now);
    assert!(claims.exp < now + 31 * 60);
}

#[test]
fn test_verify_token_malformed_string_fails() {
    let result = verify_token("not-a-valid-jwt", "secret");
    assert!(result.is_err());
}

#[test]
fn test_verify_token_empty_string_fails() {
    let result = verify_token("", "secret");
    assert!(result.is_err());
}

// ── Password Tests ───────────────────────────────────────────────────────────

#[test]
fn test_hash_and_verify_password_roundtrip() {
    let password = "super-secret-pw!";
    let hash = hash_password(password).unwrap();
    assert!(verify_password(password, &hash));
}

#[test]
fn test_verify_password_wrong_password_fails() {
    let hash = hash_password("correct-password").unwrap();
    assert!(!verify_password("wrong-password", &hash));
}

#[test]
fn test_hash_password_different_hashes_each_time() {
    // Argon2 uses random salts, so two hashes of the same password differ
    let hash1 = hash_password("same-password").unwrap();
    let hash2 = hash_password("same-password").unwrap();
    assert_ne!(hash1, hash2);
    // But both should verify
    assert!(verify_password("same-password", &hash1));
    assert!(verify_password("same-password", &hash2));
}

#[test]
fn test_verify_password_invalid_hash_string_fails() {
    assert!(!verify_password("password", "not-a-valid-argon2-hash"));
}

#[test]
fn test_verify_password_empty_password() {
    let hash = hash_password("").unwrap();
    assert!(verify_password("", &hash));
    assert!(!verify_password("not-empty", &hash));
}

#[test]
fn test_hash_password_non_ascii() {
    let password = "密码测试🔐";
    let hash = hash_password(password).unwrap();
    assert!(verify_password(password, &hash));
    assert!(!verify_password("wrong", &hash));
}
