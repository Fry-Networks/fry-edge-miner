use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

pub struct ApiClient {
    base_url: String,
    bearer_token: String,
    http: Client,
}

impl ApiClient {
    pub fn new(base_url: String, bearer_token: String) -> Self {
        Self {
            base_url,
            bearer_token,
            http: Client::new(),
        }
    }

    // All 11 hardwareapi endpoints as method stubs
    pub async fn get_required_version(
        &self,
        _miner_code: &str,
        _platform: &str,
    ) -> Result<Value> {
        todo!()
    }

    pub async fn get_supported_installers(&self, _os: &str) -> Result<Value> {
        todo!()
    }

    pub async fn get_miner_profile(&self, _miner_key: &str) -> Result<Value> {
        todo!()
    }

    pub async fn upsert_installation(
        &self,
        _miner_key: &str,
        _install_id: &str,
        _payload: Value,
    ) -> Result<()> {
        todo!()
    }

    pub async fn acquire_lease(
        &self,
        _miner_key: &str,
        _install_id: &str,
        _lease_seconds: u64,
        _external_ip: Option<&str>,
    ) -> Result<Value> {
        todo!()
    }

    pub async fn renew_lease(
        &self,
        _miner_key: &str,
        _install_id: &str,
        _lease_seconds: u64,
        _external_ip: Option<&str>,
    ) -> Result<bool> {
        todo!()
    }

    pub async fn lease_status(&self, _miner_key: &str) -> Result<Value> {
        todo!()
    }

    pub async fn delete_installation(
        &self,
        _miner_key: &str,
        _install_id: &str,
    ) -> Result<bool> {
        todo!()
    }

    pub async fn get_hardware_doc(&self, _miner_key: &str) -> Result<Value> {
        todo!()
    }

    pub async fn put_hardware_doc(
        &self,
        _miner_key: &str,
        _document: Value,
    ) -> Result<()> {
        todo!()
    }

    pub async fn check_ip_status(&self, _external_ip: &str) -> Result<Value> {
        todo!()
    }
}
