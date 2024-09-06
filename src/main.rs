use std::collections::HashMap;
use std::sync::Arc;
use std::process::Stdio;

use actix_web::{web, App, HttpResponse, HttpServer, Responder, web::Bytes};
use futures::stream::{self, StreamExt};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, broadcast::{self, Sender}};
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

pub mod rest_api {
    use serde::{Deserialize, Serialize};
    #[derive(Debug, Serialize, Deserialize)]
    pub struct Process {
        pub uuid: String,
        pub cmd: String,
        pub status: String,
        pub log: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct CreateProcess {
        pub cmd: String,
    }
}

#[derive(Clone)]
enum ProcessStatus {
    Running,
    Done(i32),
    Killed,
}

struct MyProcess {
    child: Child,
    buffer: Vec<String>,
    cmd: String,
    status: ProcessStatus,
    tx: broadcast::Sender<String>,
}

impl From<ProcessStatus> for String {
    fn from(value: ProcessStatus) -> String {
        match value {
            ProcessStatus::Running => "running".to_string(),
            ProcessStatus::Done(code) => format!("done {}", code),
            ProcessStatus::Killed => format!("killed"),
        }
    }
}

struct MyAppState {
    processes: Arc<Mutex<HashMap<String, Arc<Mutex<MyProcess>>>>>,
}

// GET /processes
async fn get_processes(data: web::Data<MyAppState>) -> impl Responder {
    let procs = stream::iter(data.processes.lock().await.iter())
        .then(|process| async move {
            let p = process.1.lock().await;
            rest_api::Process {
                uuid: process.0.clone(),
                cmd: p.cmd.clone(),
                status: p.status.clone().into(),
                log: String::new(),
            }
        })
        .collect::<Vec<_>>()
        .await;

    HttpResponse::Ok().json(procs)
}

async fn get_my_process(
    process_id: &String,
    data: &web::Data<MyAppState>,
) -> Option<Arc<Mutex<MyProcess>>> {
    let processes = data.processes.lock().await;
    let my_process_arc = processes.get(process_id)?;
    Some(my_process_arc.clone())
}

// GET /processes/{id}
async fn get_process(path: web::Path<String>, data: web::Data<MyAppState>) -> Option<HttpResponse> {
    let process_id = path.into_inner();
    let my_process_arc = get_my_process(&process_id, &data).await?;
    let my_process = my_process_arc.lock().await;
    Some(HttpResponse::Ok().json(rest_api::Process {
        uuid: process_id.clone(),
        status: my_process.status.clone().into(),
        cmd: my_process.cmd.clone(),
        log: my_process.buffer.join("\n"),
    }))
}

// GET /processes/{id}/live_log
async fn get_process_live_log(
    path: web::Path<String>,
    data: web::Data<MyAppState>,
) -> Option<impl Responder> {
    let tx: Sender<String>;
    {
        let my_process_arc = get_my_process(&path.into_inner(), &data).await?;
        tx = my_process_arc.lock().await.tx.clone();
    }
    let stream = BroadcastStream::new(tx.subscribe())
        .map(|x| Ok::<actix_web::web::Bytes, String>(Bytes::from(x.unwrap())));
    Some(
        HttpResponse::Ok()
            .content_type("text/plain")
            .streaming(stream),
    )
}

// POST /processes
async fn create_process(
    process: web::Json<rest_api::CreateProcess>,
    data: web::Data<MyAppState>,
) -> impl Responder {
    let mut cmd = Command::new(process.cmd.clone());

    cmd.stdout(Stdio::piped());
    let (tx, _) = broadcast::channel(16);
    let my_process_arc = Arc::new(Mutex::new(MyProcess {
        child: cmd.spawn().unwrap(),
        buffer: Vec::new(),
        cmd: process.cmd.clone(),
        status: ProcessStatus::Running,
        tx: tx,
    }));
    let mut processes = data.processes.lock().await;
    let new_uuid = Uuid::new_v4();
    processes.insert(new_uuid.to_string(), my_process_arc.clone());

    let mut my_process = my_process_arc.lock().await;
    let stdout = my_process.child.stdout.take().unwrap();
    let my_process_arc_spawned = my_process_arc.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Some(mut line) = reader.next_line().await.unwrap() {
            line.push('\n');
            let mut my_process = my_process_arc_spawned.lock().await;
            my_process.buffer.push(line.clone());
            let _ = my_process.tx.send(line);
        }
        let mut my_process = my_process_arc_spawned.lock().await;
        match my_process.child.wait().await.unwrap().code() {
            Some(exit_code) => {
                my_process.status = ProcessStatus::Done(exit_code);
            }
            None => {
                my_process.status = ProcessStatus::Killed;
            }
        }
    });

    HttpResponse::Created().json(rest_api::Process {
        uuid: new_uuid.to_string(),
        cmd: process.cmd.clone(),
        status: ProcessStatus::Running.into(),
        log: String::new(),
    })
}

// DELETE /processes/{id}
async fn delete_process(
    path: web::Path<String>,
    data: web::Data<MyAppState>,
) -> Option<impl Responder> {
    let process_id = path.into_inner();
    {
        let my_process_arc = get_my_process(&process_id, &data).await?;
        let mut my_process = my_process_arc.lock().await;
        if let Err(_) = my_process.child.kill().await {
            return Some(HttpResponse::InternalServerError().finish());
        }
    }
    data.processes.lock().await.remove(&process_id);
    Some(HttpResponse::NoContent().finish())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let app_state = web::Data::new(MyAppState {
        processes: Arc::new(Mutex::new(HashMap::new())),
    });
    HttpServer::new(move || {
        App::new().app_data(app_state.clone()).service(
            web::scope("/api/v1")
                .route("/processes", web::get().to(get_processes))
                .route("/processes", web::post().to(create_process))
                .route("/processes/{id}", web::get().to(get_process))
                .route(
                    "/processes/{id}/live_log",
                    web::get().to(get_process_live_log),
                )
                .route("/processes/{id}", web::delete().to(delete_process)),
        )
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
