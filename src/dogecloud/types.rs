//! DogeCloud 临时凭证和 OSS 配置类型。

use std::sync::Arc;
use tokio::sync::RwLock;

/// DogeCloud Console API 的永久密钥
#[derive(Clone, Debug)]
pub struct DogeCloudKeys {
    pub access_key: String,
    pub secret_key: String,
}

/// 从 /auth/tmp_token.json 返回的临时 S3 凭证
#[derive(Clone, Debug)]
pub struct TempCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
}

/// 多吉云返回的 Bucket 信息
#[derive(Clone, Debug)]
pub struct BucketInfo {
    pub s3_bucket: String,
    pub s3_endpoint: String,
}

/// 缓存的临时凭证（含过期时间）
pub(crate) struct CachedCredentials {
    pub credentials: TempCredentials,
    pub bucket: BucketInfo,
    /// 过期时的 Unix 时间戳（秒）
    pub expires_at: i64,
}

/// 线程安全的凭证缓存
#[derive(Clone)]
pub struct CredentialCache {
    inner: Arc<RwLock<Option<CachedCredentials>>>,
}

impl CredentialCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    /// 获取缓存的凭证，如果已过期（提前 5 分钟刷新）则返回 None
    pub async fn get(&self) -> Option<(TempCredentials, BucketInfo)> {
        let guard = self.inner.read().await;
        if let Some(cached) = guard.as_ref() {
            let now = chrono::Utc::now().timestamp();
            // 提前 5 分钟刷新
            if now < cached.expires_at - 300 {
                return Some((cached.credentials.clone(), cached.bucket.clone()));
            }
        }
        None
    }

    /// 存入新的凭证
    pub async fn set(
        &self,
        credentials: TempCredentials,
        bucket: BucketInfo,
        expires_in_secs: i64,
    ) {
        let expires_at = chrono::Utc::now().timestamp() + expires_in_secs;
        let mut guard = self.inner.write().await;
        *guard = Some(CachedCredentials {
            credentials,
            bucket,
            expires_at,
        });
    }
}
