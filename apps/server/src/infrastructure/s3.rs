//! Adaptateur S3 (Garage, ou tout service compatible) du port `ArtifactStore`.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use bytes::Bytes;
use object_store::aws::AmazonS3Builder;
use object_store::buffered::BufWriter;
use object_store::path::Path as Key;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload};
use tokio::io::AsyncWriteExt;

use crate::domain::ports::{ArtifactStore, Result};

#[derive(Debug, Clone)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

pub struct S3 {
    store: Arc<dyn ObjectStore>,
}

impl S3 {
    pub fn new(config: &S3Config) -> anyhow::Result<S3> {
        let store = AmazonS3Builder::new()
            .with_endpoint(&config.endpoint)
            .with_region(&config.region)
            .with_bucket_name(&config.bucket)
            .with_access_key_id(&config.access_key_id)
            .with_secret_access_key(&config.secret_access_key)
            .with_allow_http(config.endpoint.starts_with("http://"))
            // Garage sert les buckets sous le chemin, pas en sous-domaine.
            .with_virtual_hosted_style_request(false)
            .build()
            .context("configuration S3")?;
        Ok(S3 { store: Arc::new(store) })
    }
}

#[async_trait]
impl ArtifactStore for S3 {
    async fn put(&self, key: &str, body: Bytes) -> Result<u64> {
        let size = body.len() as u64;
        self.store
            .put(&Key::from(key), PutPayload::from(body))
            .await
            .with_context(|| format!("S3 : envoi de {key}"))?;
        Ok(size)
    }

    /// En flux, par parties : une vidéo de plusieurs centaines de Mo ne passe pas par la mémoire.
    async fn put_file(&self, key: &str, file: &Path) -> Result<u64> {
        let mut source = tokio::fs::File::open(file).await.with_context(|| file.display().to_string())?;
        let mut sink = BufWriter::new(self.store.clone(), Key::from(key));
        let size = tokio::io::copy(&mut source, &mut sink).await.with_context(|| format!("S3 : envoi de {key}"))?;
        sink.shutdown().await.with_context(|| format!("S3 : envoi de {key}"))?;
        Ok(size)
    }

    async fn get(&self, key: &str) -> Result<Bytes> {
        let result = self.store.get(&Key::from(key)).await.with_context(|| format!("S3 : lecture de {key}"))?;
        Ok(result.bytes().await?)
    }

    async fn size(&self, key: &str) -> Result<u64> {
        Ok(self.store.head(&Key::from(key)).await.with_context(|| format!("S3 : {key}"))?.size)
    }

    async fn get_range(&self, key: &str, range: Range<u64>) -> Result<Bytes> {
        Ok(self.store.get_range(&Key::from(key), range).await.with_context(|| format!("S3 : lecture de {key}"))?)
    }
}
