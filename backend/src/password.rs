use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};

use crate::error::AppResult;

fn argon2id() -> Argon2<'static> {
    // OWASP-recommended parameters for argon2id: 19 MiB, 2 iterations, 1 lane.
    let params = Params::new(19_456, 2, 1, None).expect("valid argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(password: &str) -> AppResult<String> {
    use rand::RngCore;
    let mut salt = [0u8; 16];
    rand::rng().fill_bytes(&mut salt);
    let salt = SaltString::encode_b64(&salt).map_err(|e| crate::error::AppError::internal(e))?;
    let hash = argon2id()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| crate::error::AppError::internal(e))?;
    Ok(hash.to_string())
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(p) => p,
        Err(_) => return false,
    };
    argon2id().verify_password(password.as_bytes(), &parsed).is_ok()
}
