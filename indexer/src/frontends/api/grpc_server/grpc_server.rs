use tracing::info;

use proto_buf::indexer::{ indexer_server::{ Indexer, IndexerServer }, IndexerEvent, Query };
use std::{ error::Error, time::{ SystemTime, UNIX_EPOCH } };
use tokio::sync::mpsc::channel;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{ transport::Server, Request, Response, Status };

use super::types::GRPCServerConfig;
use crate::tasks::service::TaskService;
use crate::tasks::types::TaskRecord;
use std::cmp;
use std::sync::{ Arc, Mutex };
use flume::{ Sender, Receiver, bounded };

pub struct IndexerService {
    data: Vec<TaskRecord>,
    task_service_event_receiver: Receiver<TaskRecord>,
}
pub struct GRPCServer {
    config: GRPCServerConfig,
    task_service: TaskService,
}

impl IndexerService {
    fn new(data: Vec<TaskRecord>, task_service_event_receiver: Receiver<TaskRecord>) -> Self {

        IndexerService { data, task_service_event_receiver }
    }
}

#[tonic::async_trait]
impl Indexer for IndexerService {
    type SubscribeStream = ReceiverStream<Result<IndexerEvent, Status>>;

    async fn subscribe(
        &self,
        request: Request<Query>
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let inner = request.into_inner();

        let start = SystemTime::now();
        let current_secs = start.duration_since(UNIX_EPOCH).expect("Time went backwards").as_secs();
        let offset = inner.offset;
        let limit = cmp::min(inner.offset + inner.count, self.data.len().try_into().unwrap());

        let data = self.data.clone();

        /* 
        for i in 0..10 {
            match self.task_service_event_receiver.recv() {
                Ok(msg) => println!("Received: {:?}", msg),
                Err(err) => println!("Error receiving: {}", err),
            }
        }*/

        let (tx, rx) = channel(4);
        tokio::spawn(async move {
            for i in offset..limit {
                let index: usize = i as usize;

                let record = data[index].clone();
                // info!("{:?}", record);

                let event = IndexerEvent {
                    id: i + 1,
                    schema_id: record.schema_id as u32,
                    schema_value: record.data,
                    timestamp: record.timestamp.parse::<u64>().unwrap(),
                };
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

        let task_service_event_receiver = self.task_service.event_receiver.clone();
        
        std::thread::sleep(std::time::Duration::from_millis(3000));

        let indexer_server = IndexerServer::new(IndexerService::new(data, task_service_event_receiver));
        Server::builder().add_service(indexer_server).serve(address).await?;

        Ok(())
    }
}
