use ovrcr::config::RegistryPath;
use ovrcr::protocol::{ClientMessage, Request, Response, ServerMessage, read_frame, write_frame};
use ovrcr::server::connect_if_running;
use ovrcr::server::{ServerPaths, run_server};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("server") => run_server(ServerPaths::resolve()?, RegistryPath::resolve()?.0),
        Some("list") => {
            let paths = ServerPaths::resolve()?;
            let Some(mut stream) = connect_if_running(&paths)? else {
                return Ok(());
            };
            write_frame(
                &mut stream,
                &ClientMessage {
                    request_id: 1,
                    request: Request::List,
                },
            )?;
            if let ServerMessage::Response {
                response: Response::Hierarchy(snapshot),
                ..
            } = read_frame(&mut stream)?
            {
                println!("{}", snapshot.projects.len());
            }
            Ok(())
        }
        Some("shutdown") => {
            let paths = ServerPaths::resolve()?;
            let Some(mut stream) = connect_if_running(&paths)? else {
                return Ok(());
            };
            let kill = args.next().as_deref() == Some("--kill");
            write_frame(
                &mut stream,
                &ClientMessage {
                    request_id: 1,
                    request: Request::Shutdown { kill },
                },
            )?;
            let _: ServerMessage = read_frame(&mut stream)?;
            Ok(())
        }
        _ => Ok(()),
    }
}
