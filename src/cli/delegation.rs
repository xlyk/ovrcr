use super::{AppResult, RuntimeError};
use ovrcr::protocol::{DelegationRequest, ErrorCode, Request, Response};
use std::io::Read;

pub(super) fn run() -> AppResult<()> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(524_289)
        .read_to_end(&mut bytes)
        .map_err(RuntimeError::internal)?;
    if bytes.len() > 524_288 {
        return Err(RuntimeError::new(
            ErrorCode::InvalidRequest,
            "delegation request is too large",
        ));
    }
    let request: DelegationRequest = serde_json::from_slice(&bytes)
        .map_err(|_| RuntimeError::new(ErrorCode::InvalidRequest, "invalid delegation request"))?;
    let response = super::request_without_start(Request::Delegation(Box::new(request)))?;
    match response {
        Response::Delegation(snapshot) => {
            super::print_json(&serde_json::to_value(snapshot).map_err(RuntimeError::internal)?)
        }
        Response::Error { code, message } => Err(RuntimeError::new(code, message)),
        _ => Err(RuntimeError::new(
            ErrorCode::Internal,
            "unexpected delegation response",
        )),
    }
}
