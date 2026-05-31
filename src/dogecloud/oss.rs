//! DogeCloud OSS 客户端，封装 AWS S3 SDK。
//!
//! 使用临时凭证自动刷新机制，所有 S3 操作前都会确保凭证有效。

use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::primitives::ByteStream;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::api;
use super::types::{CredentialCache, DogeCloudKeys, TempCredentials};

/// DogeCloud OSS 客户端
///
/// 持有临时凭证缓存和锁保护的 S3 Client，
/// 凭证过期时自动刷新并重建 S3 Client。
#[derive(Clone)]
pub struct DogeCloudOss {
    keys: DogeCloudKeys,
    cache: CredentialCache,
    /// 锁保护的 (Client, bucket_name)，需要在凭证刷新后重建
    inner: Arc<RwLock<Option<OssInner>>>,
    /// 配置中的 fallback CDN 参数
    fallback_bucket: String,
    fallback_endpoint: String,
}

struct OssInner {
    client: Client,
    bucket: String,
}

impl DogeCloudOss {
    /// 创建新的 OSS 客户端实例。
    ///
    /// 不会立即获取凭证，首次使用时才会拉取。
    pub fn new(config: &crate::config::AppConfig) -> Self {
        Self {
            keys: DogeCloudKeys {
                access_key: config.dogecloud_access_key.clone(),
                secret_key: config.dogecloud_secret_key.clone(),
            },
            cache: CredentialCache::new(),
            inner: Arc::new(RwLock::new(None)),
            fallback_bucket: config.dogecloud_s3_bucket.clone(),
            fallback_endpoint: config.dogecloud_s3_endpoint.clone(),
        }
    }

    /// 确保凭证有效并返回 S3 Client 和 bucket 名称。
    ///
    /// 如果凭证已过期或尚未获取，会自动调用 DogeCloud API 获取新凭证。
    async fn ensure_client(&self) -> Result<(Client, String)> {
        // 快速路径：凭证仍然有效
        if self.cache.get().await.is_some() {
            // 检查 inner 是否已初始化
            {
                let guard = self.inner.read().await;
                if let Some(ref inner) = *guard {
                    return Ok((inner.client.clone(), inner.bucket.clone()));
                }
            }
        }

        // 慢速路径：需要刷新凭证
        let mut guard = self.inner.write().await;

        // 双重检查：可能另一个 task 已经刷新了
        if self.cache.get().await.is_some() {
            if guard.is_some() {
                let inner = guard.as_ref().unwrap();
                return Ok((inner.client.clone(), inner.bucket.clone()));
            }
        }

        // 获取新凭证
        let (new_creds, mut new_bucket) = api::get_tmp_token(&self.keys).await?;

        // API 返回的值为准；仅当 API 未返回时使用配置中的 fallback
        if new_bucket.s3_bucket.is_empty() && !self.fallback_bucket.is_empty() {
            new_bucket.s3_bucket = self.fallback_bucket.clone();
        }
        if new_bucket.s3_endpoint.is_empty() && !self.fallback_endpoint.is_empty() {
            new_bucket.s3_endpoint = self.fallback_endpoint.clone();
        }

        // 缓存凭证（2小时有效期）
        self.cache
            .set(new_creds.clone(), new_bucket.clone(), 2 * 60 * 60)
            .await;

        // 构建 S3 Client
        let client = build_s3_client(&new_creds, &new_bucket.s3_endpoint).await?;

        let bucket = new_bucket.s3_bucket.clone();
        *guard = Some(OssInner {
            client: client.clone(),
            bucket: bucket.clone(),
        });

        Ok((client, bucket))
    }

    /// 上传文件。
    ///
    /// `key` 是 OSS 中的对象路径，`bytes` 是文件内容。
    pub async fn upload(&self, key: &str, bytes: Vec<u8>) -> Result<()> {
        let (client, bucket) = self.ensure_client().await?;

        client
            .put_object()
            .bucket(&bucket)
            .key(key)
            .body(ByteStream::from(bytes))
            .send()
            .await
            .context("S3 put_object failed")?;

        tracing::info!(bucket = %bucket, key = %key, "Uploaded to OSS");
        Ok(())
    }

    /// 上传本地文件。
    pub async fn upload_file(&self, key: &str, file_path: &str) -> Result<()> {
        let bytes = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("Failed to read file: {}", file_path))?;
        self.upload(key, bytes).await
    }

    /// 下载文件，返回字节内容。
    pub async fn download(&self, key: &str) -> Result<Vec<u8>> {
        let (client, bucket) = self.ensure_client().await?;

        let output = client
            .get_object()
            .bucket(&bucket)
            .key(key)
            .send()
            .await
            .context("S3 get_object failed")?;

        let data = output
            .body
            .collect()
            .await
            .context("Failed to collect S3 object body")?;

        Ok(data.into_bytes().to_vec())
    }

    /// 删除文件。
    pub async fn delete(&self, key: &str) -> Result<()> {
        let (client, bucket) = self.ensure_client().await?;

        client
            .delete_object()
            .bucket(&bucket)
            .key(key)
            .send()
            .await
            .context("S3 delete_object failed")?;

        tracing::info!(bucket = %bucket, key = %key, "Deleted from OSS");
        Ok(())
    }

    /// 列出指定前缀下的文件。
    pub async fn list(&self, prefix: Option<&str>) -> Result<Vec<String>> {
        let (client, bucket) = self.ensure_client().await?;

        let mut req = client.list_objects_v2().bucket(&bucket);
        if let Some(p) = prefix {
            req = req.prefix(p);
        }

        let output = req.send().await.context("S3 list_objects_v2 failed")?;

        let keys: Vec<String> = output
            .contents()
            .iter()
            .filter_map(|obj| obj.key().map(|k| k.to_string()))
            .collect();

        Ok(keys)
    }

    /// 获取当前 bucket 名称。
    pub async fn bucket_name(&self) -> Result<String> {
        let (_, bucket) = self.ensure_client().await?;
        Ok(bucket)
    }
}

/// 根据临时凭证和 endpoint 构建 AWS S3 Client。
async fn build_s3_client(creds: &TempCredentials, endpoint: &str) -> Result<Client> {
    let credentials = Credentials::new(
        &creds.access_key_id,
        &creds.secret_access_key,
        Some(creds.session_token.clone()),
        None,
        "dogecloud-temp-token",
    );

    let shared_config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(credentials)
        .load()
        .await;

    // endpoint 可能已包含 scheme 前缀（如 "https://cos.ap-shanghai.myqcloud.com"），
    // 也可能只有主机名（如 "cos.ap-shanghai.myqcloud.com"），统一处理。
    let endpoint_url = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_string()
    } else {
        format!("https://{endpoint}")
    };

    tracing::debug!(endpoint_url = %endpoint_url, "Building S3 client");

    let s3_config = aws_sdk_s3::config::Builder::from(&shared_config)
        .endpoint_url(endpoint_url)
        .build();

    Ok(Client::from_conf(s3_config))
}
