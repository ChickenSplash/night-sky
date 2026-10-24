//! Two warm generation workers, each limited to one CPU thread. No nested
//! all-core query per player, no unbounded work channel, no inline fallback.
use super::{Backend, MAX_RUNNING, Reply, Request};
use std::sync::{Arc, Mutex, mpsc};

type Job = (Request, mpsc::SyncSender<Reply>);

#[derive(Default)]
pub struct Native {
    send: Option<mpsc::SyncSender<Job>>,
}

impl Native {
    fn start_pool(&mut self) -> Result<(), String> {
        let (send, receive) = mpsc::sync_channel::<Job>(MAX_RUNNING);
        let receive = Arc::new(Mutex::new(receive));
        for index in 0..MAX_RUNNING {
            let receive = receive.clone();
            std::thread::Builder::new()
                .name(format!("craft generation {index}"))
                .spawn(move || {
                    let mut sky = crate::galaxy::cells::Sky::default();
                    loop {
                        let job = receive
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .recv();
                        let Ok((request, reply)) = job else {
                            break;
                        };
                        let _ = reply.send(request.answer(&mut sky, 1));
                    }
                })
                .map_err(|error| format!("Cannot start craft generation worker: {error}"))?;
        }
        self.send = Some(send);
        Ok(())
    }
}

impl Backend for Native {
    type Job = Mutex<mpsc::Receiver<Reply>>;

    fn start(&mut self, request: Request) -> Result<Self::Job, String> {
        if self.send.is_none() {
            self.start_pool()?;
        }
        let (send, receive) = mpsc::sync_channel(1);
        if let Err(error) = self.send.as_ref().unwrap().try_send((request, send)) {
            if matches!(error, mpsc::TrySendError::Disconnected(_)) {
                self.send = None;
            }
            return Err(format!("Craft generation unavailable: {error}"));
        }
        Ok(Mutex::new(receive))
    }

    fn poll(job: &mut Self::Job) -> Option<Result<Reply, String>> {
        match job
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_recv()
        {
            Ok(reply) => Some(Ok(reply)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Craft generation worker disconnected".into()))
            }
        }
    }
}
