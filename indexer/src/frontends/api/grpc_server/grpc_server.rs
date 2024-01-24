use tracing::info;
use csv::{ ReaderBuilder, StringRecord };
use std::fs::File;
use proto_buf::indexer::{ indexer_server::{ Indexer, IndexerServer }, IndexerEvent, Query };
use std::{ error::Error, time::{ SystemTime, UNIX_EPOCH } };
use tokio::sync::mpsc::channel;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{ transport::Server, Request, Response, Status };
use std::path::{ Path, PathBuf };

use super::types::GRPCServerConfig;
use crate::tasks::service::TaskService;
use crate::tasks::types::TaskRecord;
use std::cmp;
use std::sync::{ Arc, Mutex };
use flume::{ Sender, Receiver, bounded };

pub struct IndexerService {
    data: Vec<TaskRecord>,
    cache_file_path: PathBuf,
}
pub struct GRPCServer {
    config: GRPCServerConfig,
    task_service: TaskService,
}

impl IndexerService {
    fn new(data: Vec<TaskRecord>, cache_file_path: PathBuf) -> Self {
        IndexerService { data, cache_file_path }
    }
}

const DELIMITER: u8 = b';';
const CSV_COLUMN_INDEX_DATA: usize = 3;
const CSV_COLUMN_SCHEMA_ID: usize = 2;
const CSV_COLUMN_INDEX_TIMESTAMP: usize = 1;

#[tonic::async_trait]
impl Indexer for IndexerService {
    type SubscribeStream = ReceiverStream<Result<IndexerEvent, Status>>;

    async fn subscribe(
        &self,
        request: Request<Query>
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        println!("grpc requested");

        let inner = request.into_inner();

        let start = SystemTime::now();
        let current_secs = start.duration_since(UNIX_EPOCH).expect("Time went backwards").as_secs();
        let offset = inner.offset;
        let limit = cmp::min(inner.offset + inner.count, self.data.len().try_into().unwrap());

        let data = self.data.clone();

        let file_name = self.cache_file_path.clone().to_string_lossy().into_owned();

        let (tx, rx) = channel(4);
        tokio::spawn(async move {
            let file: File = File::open(file_name).unwrap();

            let mut csv_reader = ReaderBuilder::new().delimiter(DELIMITER).from_reader(file);

            for i in offset..limit {
                csv_reader.records().next();
            }

            let mut records: Vec<Result<StringRecord, csv::Error>> = csv_reader
                .into_records()
                .take(limit.try_into().unwrap())
                .collect();

            for (index, record) in records.iter().enumerate() {
                let r = record.as_ref().unwrap();

                let event = IndexerEvent {
                    id: (index as u32) + (offset as u32),
                    schema_id: r.get(CSV_COLUMN_SCHEMA_ID).unwrap().parse::<u32>().unwrap_or(0),
                    schema_value: r.get(CSV_COLUMN_INDEX_DATA).unwrap().to_string(),
                    timestamp: r
                        .get(CSV_COLUMN_INDEX_TIMESTAMP)
                        .unwrap()
                        .parse::<u64>()
                        .unwrap_or(0),
                };

                println!("{:?}", event);
                tx.send(Ok(event)).await.unwrap();
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

impl GRPCServer {
    pub fn new(config: GRPCServerConfig, task_service: TaskService) -> Self {
        GRPCServer { config, task_service }
    }

    pub async fn serve(&mut self) -> Result<(), Box<dyn Error>> {
        let address = format!("{}{}", "[::1]:", self.config.port.to_string()).parse()?;
        info!("GRPC server is starting at {}", address);
        self.task_service.run().await;

        // todo
        let data = self.task_service.get_chunk(0, 10000).await;
        // todo move to cache layer
        let cache_file_path = self.task_service.get_cache_file_path();

        // let task_service_event_receiver = self.task_service.event_receiver.clone();

        std::thread::sleep(std::time::Duration::from_millis(3000));

        let indexer_server = IndexerServer::new(IndexerService::new(data, cache_file_path));
        Server::builder().add_service(indexer_server).serve(address).await?;

        Ok(())
    }
}
