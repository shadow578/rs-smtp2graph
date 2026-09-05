use crate::app::run_mail_proxy;
use crate::cli::{Cli, CliCommand};
use anyhow::Result;
use clap::Parser;
use log::{debug, error};
use std::ffi::OsString;
use std::time::Duration;
use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType};
use windows_service::service_control_handler::ServiceControlHandlerResult;
use windows_service::{define_windows_service, service_control_handler, service_dispatcher};

const SERVICE_NAME: &str = "postgraph";

define_windows_service!(ffi_service_main, service_main);

/// dispatch running the mail proxy as a Windows service.
/// this will start up the service and block until it exits.
pub(crate) fn dispatch_service() -> Result<()> {
    debug!("dispatching service start");
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

/// service_main called by service dispatcher.
fn service_main(_args: Vec<OsString>) {
    if let Err(err) = run_service() {
        error!("error during run_service: {:?}", err);
    }
}

/// core service logic: handle state reporting, run mail_proxy async and then shutdown.
fn run_service() -> Result<()> {
    debug!("running service");

    // register control handle with shutdown handler
    // windows handler will signal async runtime through shutdown_rx and shutdown_tx to exit
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    let status_handle = service_control_handler::register(
        SERVICE_NAME,
        move |event| {
            match event {
                ServiceControl::Stop => {
                    debug!("received stop service control, signaling shutdown to async...");
                    let _ = shutdown_tx.send(());
                    ServiceControlHandlerResult::NoError
                }
                // all services must accept Interrogate, but it may be a no-op.
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        },
    )?;

    // report service has started (kinda a lie, but oh well)
    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    // actually start doing stuff
    // get cli args passed to the binary (from argv, not service_main; the latter are a separate thing)
    // if cli command indicates we should run as a service, do so
    // in all other cases, wa fall through and immediately exit the service
    let cli = Cli::parse();
    let exit_code = if let Some(CliCommand::Run { service: true }) = cli.command
    {
        let result = tokio::runtime::Runtime::new()?
            .block_on(async {
                tokio::select! {
                    result = run_mail_proxy(&cli) => {
                        debug!("run_mail_proxy exited with {:?}", result);
                        result
                    },
                    _ = tokio::task::spawn_blocking(move || { let _ = shutdown_rx.recv(); }) => {
                        debug!("received shutdown signal in async, stopping");
                        Ok(())
                    }
                }
            });

        if result.is_ok() { 0 } else { 0x8001 }
    } else {
        0x8101
    };

    // report service has stopped and will now exit
    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(exit_code),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;
    Ok(())
}
