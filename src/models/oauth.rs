use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OAuthSessionData {
    pub pkce_verifier_secret: Option<String>,
    pub csrf_token_secret: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}
