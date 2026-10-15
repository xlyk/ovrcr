use super::{AppResult, RuntimeError};
use ovrcr::protocol::{DelegationRequest, WorkflowRequest, ErrorCode, Request, Response};
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
    let request = match serde_json::from_slice::<WorkflowRequest>(&bytes) {
        Ok(request) => Request::Workflow(Box::new(request)),
        Err(_) => Request::Delegation(Box::new(serde_json::from_slice::<DelegationRequest>(&bytes)
            .map_err(|_| RuntimeError::new(ErrorCode::InvalidRequest, "invalid delegation request"))?)),
    };
    let response = super::request_without_start(request)?;
    match response {
        Response::Workflows(workflows) => super::print_json(&serde_json::to_value(workflows).map_err(RuntimeError::internal)?),
        Response::Delegation(snapshot) => {
            super::print_json(&serde_json::to_value(snapshot).map_err(RuntimeError::internal)?)
        }
        Response::DelegationObservation(batch) => {
            super::print_json(&serde_json::to_value(batch).map_err(RuntimeError::internal)?)
        }
        Response::DelegationCapabilities(capabilities) => {
            super::print_json(&serde_json::to_value(capabilities).map_err(RuntimeError::internal)?)
        }
        Response::Error { code, message } => Err(RuntimeError::new(code, message)),
        _ => Err(RuntimeError::new(
            ErrorCode::Internal,
            "unexpected delegation response",
        )),
    }
}
