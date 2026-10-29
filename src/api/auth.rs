use std::sync::Arc;

use poem::Request;
use poem_openapi::{auth::Bearer, SecurityScheme};

/// Bearer API Key（与启动时 `--api-key` / `IDORC_API_KEY` 比对）。
#[derive(SecurityScheme)]
#[oai(ty = "bearer", checker = "check_bearer")]
pub struct BearerAuth(());

async fn check_bearer(req: &Request, token: Bearer) -> Option<()> {
    let expected = req.data::<Arc<String>>()?;
    (token.token == expected.as_str()).then_some(())
}
