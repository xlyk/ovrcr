//! `ovrcr operator` — the dedicated Pi profile, its model list, and the hidden worker.
use super::{AppResult, RuntimeError, args::OperatorCommand, output::print_json};
use ovrcr::operator::pi;
use ovrcr::protocol::{
    ErrorCode, OperatorConversationId, OperatorRunIdentity, SessionId, SessionRunId,
};
use serde_json::json;
use std::io::IsTerminal;

pub(super) fn run(command: OperatorCommand, json_output: bool) -> AppResult<()> {
    match command {
        OperatorCommand::Worker {
            conversation,
            session,
            run,
        } => ovrcr::operator::worker::run(OperatorRunIdentity {
            conversation: OperatorConversationId(conversation),
            session: SessionId(session),
            run: SessionRunId(run),
        })
        .map_err(RuntimeError::internal),
        OperatorCommand::Setup => setup(json_output),
        OperatorCommand::Models => models(json_output),
    }
}

/// Native pi owns sign-in, model choice and sign-out; OVRCR only prepares the profile.
fn setup(json_output: bool) -> AppResult<()> {
    if json_output {
        return Err(RuntimeError::new(
            ErrorCode::InvalidRequest,
            "ovrcr operator setup is interactive; drop --json to run it",
        ));
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(RuntimeError::new(
            ErrorCode::InvalidRequest,
            "ovrcr operator setup needs an interactive terminal",
        ));
    }
    eprintln!(
        "OVRCR Operator uses a dedicated Pi profile, separate from your own Pi account.\n\
         \n\
         In the Pi session that starts next:\n\
         \n  /login    sign in to a subscription account, or refresh it\n  \
         /model    choose this profile's default model\n  \
         /logout   remove the account from this profile\n\
         \n\
         Then leave the session. Operator keeps using this conversation's model\n\
         unless a model is chosen in OVRCR settings."
    );
    pi::run_setup().map_err(|error| match error.kind() {
        pi::PiErrorKind::Unavailable => {
            RuntimeError::new(ErrorCode::Internal, error.user_message())
        }
        // Setup is the one place a native launch failure is not a turn failure.
        _ => RuntimeError::new(
            ErrorCode::Internal,
            "Could not start native Pi setup; install pi 0.87.1 and run it again.",
        ),
    })
}

fn models(json_output: bool) -> AppResult<()> {
    let models = pi::list_models()
        .map_err(|error| RuntimeError::new(ErrorCode::Internal, error.user_message()))?;
    if json_output {
        let entries: Vec<_> = models
            .iter()
            .map(|model| {
                json!({
                    "provider": model.provider,
                    "id": model.id,
                    "name": model.name,
                })
            })
            .collect();
        print_json(&json!({ "models": entries }))?;
    } else if !models.is_empty() {
        for model in &models {
            println!("{}/{}\t{}", model.provider, model.id, model.name);
        }
    }
    if models.is_empty() {
        // An empty native list is empty; never invent default identifiers.
        eprintln!("No Pi model selected; run ovrcr operator setup.");
    }
    Ok(())
}
