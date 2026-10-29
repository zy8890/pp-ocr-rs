use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use tokio::sync::Semaphore;

use crate::engine::{IdCardOcr, OcrEngineOptions, RecognizeTimings};
use crate::id_card::IdCardResult;

/// 固定数量 OCR 引擎池；HTTP 层并发，推理受 semaphore 限制。
pub struct OcrPool {
    engines: Arc<Vec<Mutex<IdCardOcr>>>,
    sem: Arc<Semaphore>,
    workers: usize,
}

impl OcrPool {
    pub fn new(options: OcrEngineOptions, workers: usize) -> Result<Self> {
        let workers = workers.max(1);
        let mut engines = Vec::with_capacity(workers);
        for i in 0..workers {
            let eng = IdCardOcr::new(options.clone())
                .with_context(|| format!("初始化 OCR worker {i} 失败"))?;
            engines.push(Mutex::new(eng));
        }
        Ok(Self {
            engines: Arc::new(engines),
            sem: Arc::new(Semaphore::new(workers)),
            workers,
        })
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    pub async fn recognize_bytes(
        &self,
        bytes: Vec<u8>,
    ) -> Result<(IdCardResult, RecognizeTimings)> {
        let _permit = self
            .sem
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow!("OCR semaphore closed"))?;
        let engines = Arc::clone(&self.engines);
        tokio::task::spawn_blocking(move || {
            // permit 保证有空闲槽；优先 try_lock，否则阻塞锁第一把
            for m in engines.iter() {
                if let Ok(mut g) = m.try_lock() {
                    return g.recognize_id_card_bytes_timed(&bytes);
                }
            }
            let mut g = engines[0]
                .lock()
                .map_err(|_| anyhow!("OCR worker mutex poisoned"))?;
            g.recognize_id_card_bytes_timed(&bytes)
        })
        .await
        .context("OCR spawn_blocking join failed")?
    }
}
