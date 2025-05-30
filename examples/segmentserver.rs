use std::{env, io};

use axum::{extract::Path, response::IntoResponse, routing::get, Router};

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage: segmentserver <filename>");
        std::process::exit(1);
    }

    

}