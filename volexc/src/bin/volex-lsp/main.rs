mod lsp;

use std::error::Error;

use lsp_server::{Connection, Message, Request, RequestId, Response};
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Starting Volex LSP server");

    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::FULL),
            will_save: None,
            will_save_wait_until: None,
            save: Some(TextDocumentSyncSaveOptions::Supported(true)),
        })),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        ..Default::default()
    })
    .unwrap();

    let initialization_params = connection.initialize(server_capabilities)?;
    main_loop(connection, initialization_params)?;
    io_threads.join()?;

    log::info!("Shutting down Volex LSP server");
    Ok(())
}

fn main_loop(connection: Connection, _params: serde_json::Value) -> Result<(), Box<dyn Error + Sync + Send>> {
    let mut lsp_state = lsp::VolexLsp::new();

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                handle_request(&connection, &mut lsp_state, req)?;
            }
            Message::Notification(not) => {
                handle_notification(&connection, &mut lsp_state, not)?;
            }
            Message::Response(_resp) => {
                // We don't send requests, so we don't expect responses
            }
        }
    }
    Ok(())
}

fn handle_request(
    connection: &Connection,
    lsp_state: &mut lsp::VolexLsp,
    req: Request,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let req = match cast_request::<GotoDefinition>(req) {
        Ok((id, params)) => {
            log::info!("Got goto definition request #{id}: {params:?}");
            let result = lsp_state.goto_definition(params);
            let result = serde_json::to_value(result).unwrap();
            let resp = Response {
                id,
                result: Some(result),
                error: None,
            };
            connection.sender.send(Message::Response(resp))?;
            return Ok(());
        }
        Err(req) => req,
    };

    let req = match cast_request::<HoverRequest>(req) {
        Ok((id, params)) => {
            log::info!("Got hover request #{id}: {params:?}");
            let result = lsp_state.hover(params);
            let result = serde_json::to_value(result).unwrap();
            let resp = Response {
                id,
                result: Some(result),
                error: None,
            };
            connection.sender.send(Message::Response(resp))?;
            return Ok(());
        }
        Err(req) => req,
    };

    let req = match cast_request::<Rename>(req) {
        Ok((id, params)) => {
            log::info!("Got rename request #{id}: {params:?}");
            let result = lsp_state.rename(params);
            let result = serde_json::to_value(result).unwrap();
            let resp = Response {
                id,
                result: Some(result),
                error: None,
            };
            connection.sender.send(Message::Response(resp))?;
            return Ok(());
        }
        Err(req) => req,
    };

    log::warn!("Unhandled request: {req:?}");
    Ok(())
}

fn handle_notification(
    connection: &Connection,
    lsp_state: &mut lsp::VolexLsp,
    not: lsp_server::Notification,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let not = match cast_notification::<DidOpenTextDocument>(not) {
        Ok(params) => {
            log::info!("Got didOpen notification: {:?}", params.text_document.uri);
            lsp_state.did_open(params.clone());

            // Send diagnostics
            let diagnostics = lsp_state.diagnostics(&params.text_document.uri);
            send_diagnostics(connection, params.text_document.uri, diagnostics)?;
            return Ok(());
        }
        Err(not) => not,
    };

    let not = match cast_notification::<DidChangeTextDocument>(not) {
        Ok(params) => {
            log::info!("Got didChange notification: {:?}", params.text_document.uri);
            lsp_state.did_change(params.clone());

            // Send diagnostics
            let diagnostics = lsp_state.diagnostics(&params.text_document.uri);
            send_diagnostics(connection, params.text_document.uri, diagnostics)?;
            return Ok(());
        }
        Err(not) => not,
    };

    let not = match cast_notification::<DidCloseTextDocument>(not) {
        Ok(params) => {
            log::info!("Got didClose notification: {:?}", params.text_document.uri);
            lsp_state.did_close(params);
            return Ok(());
        }
        Err(not) => not,
    };

    let not = match cast_notification::<DidSaveTextDocument>(not) {
        Ok(params) => {
            log::info!("Got didSave notification: {:?}", params.text_document.uri);
            // Diagnostics already sent on change
            return Ok(());
        }
        Err(not) => not,
    };

    log::warn!("Unhandled notification: {not:?}");
    Ok(())
}

fn send_diagnostics(
    connection: &Connection,
    uri: Url,
    diagnostics: Vec<Diagnostic>,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let params = PublishDiagnosticsParams {
        uri,
        diagnostics,
        version: None,
    };
    let notification = lsp_server::Notification {
        method: "textDocument/publishDiagnostics".to_string(),
        params: serde_json::to_value(params).unwrap(),
    };
    connection.sender.send(Message::Notification(notification))?;
    Ok(())
}

fn cast_request<R>(req: Request) -> Result<(RequestId, R::Params), Request>
where
    R: lsp_types::request::Request,
    R::Params: serde::de::DeserializeOwned,
{
    if req.method == R::METHOD {
        let params = serde_json::from_value(req.params).unwrap();
        Ok((req.id, params))
    } else {
        Err(req)
    }
}

fn cast_notification<N>(not: lsp_server::Notification) -> Result<N::Params, lsp_server::Notification>
where
    N: lsp_types::notification::Notification,
    N::Params: serde::de::DeserializeOwned,
{
    if not.method == N::METHOD {
        let params = serde_json::from_value(not.params).unwrap();
        Ok(params)
    } else {
        Err(not)
    }
}
