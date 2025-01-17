mod store;

use store::*;
use tower_http::services::ServeFile;

use std::{collections::HashMap, fs, sync::Arc};

use axum::{
    extract::{Multipart, Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Result},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use tracing::{event, instrument, Level};

#[derive(Parser, Debug)]
#[command(version, about, long_about=None)]
struct Args {
    #[arg(long)]
    state_dir: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let shared_state = Arc::new(Store::new(args.state_dir));

    tracing_subscriber::fmt::init();
    let app = Router::new()
        .route("/projects", get(get_projects))
        .route("/project/{project}/versions", get(get_versions))
        .route(
            "/project/{project}/version/{version}/download",
            get(get_version),
        )
        .route(
            "/project/{project}/version/{version}/file/{file}",
            get(get_version_content),
        )
        .route("/project/{project}/upload", post(new_version))
        .with_state(shared_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn get_projects(State(store): State<Arc<Store>>) -> Result<Json<Vec<String>>> {
    Ok(Json(store.list_projects()?))
}

async fn get_versions(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>> {
    let project = store.project_reader(project, &headers)?;
    Ok(Json(store.list_versions(&project)?))
}

async fn get_version(
    State(store): State<Arc<Store>>,
    Path((project, version)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    let project = store.project_reader(project, &headers)?;
    let version = Version::new(version)?;
    let file = store.file_for_version(&project, &version)?;
    Ok(Redirect::to(&format!(
        "/project/{}/version/{}/file/{}",
        &project.name(),
        &version.name(),
        file
    )))
}

async fn get_version_content(
    State(store): State<Arc<Store>>,
    Path((project, version, given_file)): Path<(String, String, String)>,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Result<impl IntoResponse> {
    let project = store.project_reader(project, &headers)?;
    let version = Version::new(version)?;
    let file = store.file_for_version(&project, &version)?;
    if file != given_file {
        return Err(StoreError::InvalidFile.into());
    }
    let path = store.path_for_version(&project, &version)?;
    ServeFile::new(&path)
        .try_call(req)
        .await
        .map_err(StoreError::IO)
        .map_err(|e| e.into())
}

async fn new_version(
    State(store): State<Arc<Store>>,
    Path(project): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<impl IntoResponse> {
    let project = store.project_writer(project, &headers)?;
    let version = match params.get("version") {
        Some(v) => Version::new(v.clone()),
        None => Err(StoreError::Other("did not provide version".to_string())),
    }?;
    let mut got_file = false;
    while let Some(field) = multipart.next_field().await? {
        let file_name = match field.file_name() {
            Some(file_name) => file_name,
            None => continue,
        };
        let outpath = store.outpath_for(&project, &version, file_name)?;
        let bytes = field.bytes().await?;
        fs::write(outpath, bytes).map_err(StoreError::IO)?;
        got_file = true;
    }
    if got_file {
        event!(
            Level::INFO,
            "uploaded version {} for project {}",
            version.name(),
            project.name()
        );
        Ok(format!(
            "successful upload of version {} for project {}",
            version.name(),
            project.name()
        ))
    } else {
        Err(StoreError::Other("failed to upload".to_string()).into())
    }
}
