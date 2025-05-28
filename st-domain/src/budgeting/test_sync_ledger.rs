use crate::budgeting::treasury_redesign::{LedgerArchiveTask, LedgerArchiveTaskSender, LedgerEntry};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct TestLedgerArchiver {
    entries: Arc<Mutex<VecDeque<LedgerEntry>>>,
}

impl TestLedgerArchiver {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn get_entries(&self) -> Vec<LedgerEntry> {
        self.entries.lock().unwrap().iter().cloned().collect()
    }

    pub fn get_entry_count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    fn process_entry(&self, entry: LedgerEntry) -> anyhow::Result<()> {
        self.entries.lock().unwrap().push_back(entry);
        Ok(())
    }
}

pub async fn create_test_ledger_setup() -> (TestLedgerArchiver, LedgerArchiveTaskSender) {
    let (task_sender, mut task_receiver) = tokio::sync::mpsc::unbounded_channel::<LedgerArchiveTask>();
    let archiver = TestLedgerArchiver::new();
    let archiver_clone = archiver.clone();

    tokio::spawn(async move {
        println!("Archiver task started, waiting for tasks...");
        while let Some(task) = task_receiver.recv().await {
            println!("Received task for entry: {:?}", task.entry);
            let result = archiver_clone.process_entry(task.entry);
            println!("Processed entry, sending response: {:?}", result);

            match task.response_sender.send(result).await {
                Ok(_) => println!("Successfully sent response"),
                Err(e) => println!("Failed to send response: {:?}", e),
            }
        }
        println!("Archiver task ended");
    });

    tokio::task::yield_now().await;
    (archiver, task_sender)
}
