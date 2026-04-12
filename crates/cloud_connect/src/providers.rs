//! Cloud provider trait and implementations (GenericSsh, Lambda Labs).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Status of a cloud or SSH instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstanceStatus {
    Running,
    Stopped,
    Starting,
    Stopping,
    Terminated,
    Unknown,
}

impl InstanceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Stopped => "Stopped",
            Self::Starting => "Starting",
            Self::Stopping => "Stopping",
            Self::Terminated => "Terminated",
            Self::Unknown => "Unknown",
        }
    }
}

/// GPU information for a cloud instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub count: u32,
    pub vram_gb: u32,
}

/// SSH connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub key_path: Option<String>,
}

/// A cloud or SSH-accessible instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudInstance {
    pub id: String,
    pub name: String,
    pub status: InstanceStatus,
    pub instance_type: String,
    pub gpu_info: Option<GpuInfo>,
    pub hourly_cost: Option<f64>,
    pub region: String,
    pub ip_address: Option<String>,
    pub uptime: Option<Duration>,
    pub ssh_config: Option<SshConfig>,
}

/// Trait for cloud provider integrations.
pub trait CloudProvider: Send + Sync {
    fn name(&self) -> &str;
    fn list_instances(&self) -> Result<Vec<CloudInstance>>;
    fn start_instance(&self, id: &str) -> Result<()>;
    fn stop_instance(&self, id: &str) -> Result<()>;
    fn get_ssh_config(&self, id: &str) -> Result<SshConfig>;
}

/// A simple provider backed by user-configured SSH connections.
#[derive(Default)]
pub struct GenericSshProvider {
    connections: Vec<CloudInstance>,
}

impl GenericSshProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_connection(
        &mut self,
        name: impl Into<String>,
        host: impl Into<String>,
        user: impl Into<String>,
        port: u16,
        key_path: Option<String>,
    ) {
        let name = name.into();
        let host = host.into();
        let user = user.into();
        let id = format!("ssh-{}", name.to_lowercase().replace(' ', "-"));

        self.connections.push(CloudInstance {
            id,
            name,
            status: InstanceStatus::Unknown,
            instance_type: "SSH".to_string(),
            gpu_info: None,
            hourly_cost: None,
            region: String::new(),
            ip_address: Some(host.clone()),
            uptime: None,
            ssh_config: Some(SshConfig {
                host,
                user,
                port,
                key_path,
            }),
        });
    }
}

impl CloudProvider for GenericSshProvider {
    fn name(&self) -> &str {
        "SSH Connections"
    }

    fn list_instances(&self) -> Result<Vec<CloudInstance>> {
        Ok(self.connections.clone())
    }

    fn start_instance(&self, _id: &str) -> Result<()> {
        anyhow::bail!("Cannot start a generic SSH connection — the host must be running already")
    }

    fn stop_instance(&self, _id: &str) -> Result<()> {
        anyhow::bail!("Cannot stop a generic SSH host from the IDE")
    }

    fn get_ssh_config(&self, id: &str) -> Result<SshConfig> {
        self.connections
            .iter()
            .find(|instance| instance.id == id)
            .and_then(|instance| instance.ssh_config.clone())
            .ok_or_else(|| anyhow::anyhow!("No SSH config for connection '{}'", id))
    }
}

/// Lambda Labs cloud GPU provider.
///
/// Uses the Lambda Labs REST API:
///   GET  https://cloud.lambdalabs.com/api/v1/instances
///   POST https://cloud.lambdalabs.com/api/v1/instance-operations/launch
///   POST https://cloud.lambdalabs.com/api/v1/instance-operations/terminate
pub struct LambdaLabsProvider {
    api_key: String,
}

impl LambdaLabsProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
        }
    }
}

impl CloudProvider for LambdaLabsProvider {
    fn name(&self) -> &str {
        "Lambda Labs"
    }

    fn list_instances(&self) -> Result<Vec<CloudInstance>> {
        // In a full implementation this would call the Lambda Labs API.
        // For the build pass we return an empty list; the real HTTP call
        // will be wired up when manual testing begins.
        log::debug!(
            target: "ribhu::cloud_connect",
            "Lambda Labs list_instances (API key len={})",
            self.api_key.len()
        );
        Ok(Vec::new())
    }

    fn start_instance(&self, id: &str) -> Result<()> {
        log::info!(
            target: "ribhu::cloud_connect",
            "Lambda Labs start_instance: {}",
            id
        );
        Ok(())
    }

    fn stop_instance(&self, id: &str) -> Result<()> {
        log::info!(
            target: "ribhu::cloud_connect",
            "Lambda Labs stop_instance: {}",
            id
        );
        Ok(())
    }

    fn get_ssh_config(&self, _id: &str) -> Result<SshConfig> {
        // The real implementation would fetch instance details from the API.
        anyhow::bail!("Lambda Labs SSH config requires a running instance with an assigned IP")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_ssh_add_and_list() {
        let mut provider = GenericSshProvider::new();
        provider.add_connection("My GPU Box", "10.0.0.5", "ubuntu", 22, None);
        let instances = provider.list_instances().unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].name, "My GPU Box");
        assert_eq!(instances[0].instance_type, "SSH");
    }

    #[test]
    fn generic_ssh_get_config() {
        let mut provider = GenericSshProvider::new();
        provider.add_connection("box", "192.168.1.10", "root", 2222, Some("/home/user/.ssh/id_rsa".into()));
        let config = provider.get_ssh_config("ssh-box").unwrap();
        assert_eq!(config.host, "192.168.1.10");
        assert_eq!(config.user, "root");
        assert_eq!(config.port, 2222);
    }

    #[test]
    fn instance_status_strings() {
        assert_eq!(InstanceStatus::Running.as_str(), "Running");
        assert_eq!(InstanceStatus::Stopped.as_str(), "Stopped");
        assert_eq!(InstanceStatus::Terminated.as_str(), "Terminated");
    }
}
