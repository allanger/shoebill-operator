#![allow(unused_imports, unused_variables)]
use std::process::exit;

use actix_web::{
    get, middleware, web::Data, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use clap::{Args, Command, Parser, Subcommand};
use cmd::{Cli, Commands};
use controllers::configsets_controller;
use log::*;
mod api;
mod cmd;
mod controllers;
mod helpers;

#[get("/")]
async fn index(req: HttpRequest) -> impl Responder {
    let d = "Shoebill";
    HttpResponse::Ok().json(&d)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match &cli.command {
        Commands::Manifests(args) => helpers::manifests::generate_kube_manifests(
            args.namespace.clone(),
            args.image.clone(),
            args.tag.clone(),
        ),
        Commands::Controller(args) => {
            // Initiatilize Kubernetes controller state
            let controller = configsets_controller::setup();
            // Start web server
            let server =
                match HttpServer::new(move || App::new().service(index)).bind("0.0.0.0:8080") {
                    Ok(server) => server.shutdown_timeout(5),
                    Err(err) => {
                        error!("{}", err);
                        exit(1)
                    }
                };
            // Both runtimes implements graceful shutdown, so poll until both are done
            match tokio::join!(controller, server.run()).1 {
                Ok(res) => info!("server is started"),
                Err(err) => {
                    error!("{}", err);
                    exit(1)
                }
            };
        }
    }

    Ok(())
}
