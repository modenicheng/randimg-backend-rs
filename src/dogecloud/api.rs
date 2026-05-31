//! DogeCloud Console API 客户端。
//!
//! 参考: https://docs.dogecloud.com/oss/sdk-introduction

use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use reqwest::Client;
use sha1::Sha1;
use std::collections::HashMap;

use super::types::{BucketInfo, DogeCloudKeys, TempCredentials};

type HmacSha1 = Hmac<Sha1>;

const DOGECLOUD_API_BASE: &str = "https://api.dogecloud.com";

/// 调用多吉云 Console API，返回 JSON。
///
/// 逻辑与原 Python `dogecloud_api()` 完全对应：
/// - 签名 = HMAC-SHA1(secret_key, api_path + "\n" + body)
/// - Authorization: TOKEN {access_key}:{sign_hex}
async fn dogecloud_api(
    keys: &DogeCloudKeys,
    api_path: &str,
    data: Option<&HashMap<String, String>>,
    json_mode: bool,
) -> Result<serde_json::Value> {
    let (body, content_type) = if json_mode {
        let body = serde_json::to_string(data.unwrap_or(&HashMap::new()))?;
        (body, "application/json")
    } else {
        let body = if let Some(d) = data {
            serde_urlencoded::to_string(d)?
        } else {
            String::new()
        };
        (body, "application/x-www-form-urlencoded")
    };

    let sign_str = format!("{}\n{}", api_path, body);
    let mut mac =
        HmacSha1::new_from_slice(keys.secret_key.as_bytes()).context("HMAC init failed")?;
    mac.update(sign_str.as_bytes());
    let sign = hex::encode(mac.finalize().into_bytes());
    let authorization = format!("TOKEN {}:{}", keys.access_key, sign);

    let url = format!("{}{}", DOGECLOUD_API_BASE, api_path);
    let client = Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", authorization)
        .header("Content-Type", content_type)
        .body(body)
        .send()
        .await
        .context("DogeCloud API request failed")?;

    let json: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse DogeCloud API response")?;

    Ok(json)
}

/// 获取临时 S3 凭证。
///
/// 对应原 Python `get_tmp_token()`：
/// POST /auth/tmp_token.json
///   channel=OSS_FULL, scopes=["*"]
pub async fn get_tmp_token(keys: &DogeCloudKeys) -> Result<(TempCredentials, BucketInfo)> {
    let mut data = HashMap::new();
    data.insert("channel".to_string(), "OSS_FULL".to_string());
    data.insert("scopes".to_string(), "*".to_string());

    let resp = dogecloud_api(keys, "/auth/tmp_token.json", Some(&data), true).await?;

    let code = resp["code"].as_i64().unwrap_or(0);
    if code != 200 {
        let msg = resp["msg"].as_str().unwrap_or("unknown error");
        anyhow::bail!("DogeCloud tmp_token API failed (code {}): {}", code, msg);
    }

    let creds = &resp["data"]["Credentials"];
    let access_key_id = creds["accessKeyId"]
        .as_str()
        .context("missing accessKeyId")?
        .to_string();
    let secret_access_key = creds["secretAccessKey"]
        .as_str()
        .context("missing secretAccessKey")?
        .to_string();
    let session_token = creds["sessionToken"]
        .as_str()
        .context("missing sessionToken")?
        .to_string();

    let bucket_info = &resp["data"]["Buckets"][0];
    let s3_bucket = bucket_info["s3Bucket"]
        .as_str()
        .context("missing s3Bucket")?
        .to_string();
    let s3_endpoint = bucket_info["s3Endpoint"]
        .as_str()
        .context("missing s3Endpoint")?
        .to_string();

    tracing::info!(
        bucket = %s3_bucket,
        endpoint = %s3_endpoint,
        "Fetched DogeCloud temporary credentials"
    );

    Ok((
        TempCredentials {
            access_key_id,
            secret_access_key,
            session_token,
        },
        BucketInfo {
            s3_bucket,
            s3_endpoint,
        },
    ))
}
