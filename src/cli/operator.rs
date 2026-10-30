use super::{AppResult, RuntimeError, args::OperatorCommand, output::print_json};
use ovrcr::operator::{
    auth::{AuthCancel, AuthManager},
    login::BrowserRedirect,
};
use ovrcr::protocol::{
    OperatorConversationId, OperatorProvider, OperatorTurnId, OperatorTurnIdentity, SessionId,
    SessionRunId,
};
use serde_json::json;

pub(super) fn run(command: OperatorCommand, json_output: bool) -> AppResult<()> {
    if let OperatorCommand::Worker {
        conversation,
        turn,
        session,
        run,
    } = command
    {
        return ovrcr::operator::worker::run(OperatorTurnIdentity {
            conversation: OperatorConversationId(conversation),
            turn: OperatorTurnId(turn),
            session: SessionId(session),
            run: SessionRunId(run),
        })
        .map_err(RuntimeError::internal);
    }
    let instance = ovrcr::config::RegistryPath::resolve()
        .map_err(RuntimeError::internal)?
        .0;
    std::fs::create_dir_all(&instance).map_err(RuntimeError::internal)?;
    let auth = AuthManager::new(&instance).map_err(RuntimeError::internal)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(RuntimeError::internal)?;
    runtime.block_on(async {
        let cancel = AuthCancel::default();
        match command {
            OperatorCommand::Status { provider } => {
                let provider: OperatorProvider = provider.into();
                let status = auth.status(provider).await.map_err(RuntimeError::internal)?;
                if json_output {
                    print_json(&json!({"provider": provider.as_str(), "signed_in":status.signed_in,
                        "needs_refresh":status.needs_refresh, "plan_usage":status.plan_usage,
                        "subject":status.subject, "client_id":status.client_id}))
                } else {
                    println!("{}: {}{}", provider.as_str(), if status.signed_in { "signed in" } else { "signed out" },
                        if status.needs_refresh { " (refresh needed)" } else { "" });
                    Ok(())
                }
            }
            OperatorCommand::Models { provider } => {
                let provider: OperatorProvider = provider.into();
                let models = auth.models(provider, &cancel).await.map_err(RuntimeError::internal)?;
                if json_output { print_json(&json!({"provider":provider.as_str(), "models": models})) }
                else { for model in models { println!("{}\t{}", model.id, model.display_name); } Ok(()) }
            }
            OperatorCommand::Logout { provider } => {
                let result = auth.logout(provider.into(), &cancel).await.map_err(RuntimeError::internal)?;
                if json_output { print_json(&json!({"signed_in":false, "remote_revocation_confirmed": result.remote_revocation_confirmed})) }
                else { println!("Signed out locally. Remote revocation {}.", if result.remote_revocation_confirmed { "confirmed" } else { "unconfirmed" }); Ok(()) }
            }
            OperatorCommand::Login { provider } => {
                match OperatorProvider::from(provider) {
                    OperatorProvider::ChatGpt => {
                        let login = auth.begin_chatgpt().await.map_err(RuntimeError::internal)?;
                        let redirect = BrowserRedirect::new(login.authorization_url()).await.map_err(RuntimeError::internal)?;
                        eprintln!("Complete ChatGPT subscription sign-in in your browser.");
                        redirect.open().await.map_err(RuntimeError::internal)?;
                        auth.finish_chatgpt(login, &cancel).await.map_err(RuntimeError::internal)?;
                    }
                    OperatorProvider::Grok => {
                        let login = auth.begin_grok(&cancel).await.map_err(RuntimeError::internal)?;
                        eprintln!("Enter code {} at {}", login.user_code, login.verification_uri);
                        // Use the same secret-free launcher path; no provider URL enters argv.
                        let redirect = BrowserRedirect::new(&login.verification_uri).await.map_err(RuntimeError::internal)?;
                        redirect.open().await.map_err(RuntimeError::internal)?;
                        auth.finish_grok(login, &cancel).await.map_err(RuntimeError::internal)?;
                    }
                }
                if json_output { print_json(&json!({"signed_in":true})) } else { println!("Subscription sign-in complete."); Ok(()) }
            }
            OperatorCommand::Worker { .. } => unreachable!(),
        }
    })
}
