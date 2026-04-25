//! System metrics data types and parsing for remote resource collection.
//!
//! The actual data comes from the Ribhu remote agent which reads /proc/stat,
//! /proc/meminfo, nvidia-smi, df, and ps on the remote machine and sends
//! the metrics back over the SSH tunnel as JSON.

use serde::{Deserialize, Serialize};

/// Snapshot of all system metrics at a point in time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub cpu_percent: f32,
    pub cpu_cores: u32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    pub gpu: Option<GpuMetrics>,
    pub disk_used_gb: f32,
    pub disk_total_gb: f32,
    pub net_up_mbps: f32,
    pub net_down_mbps: f32,
    pub top_processes: Vec<ProcessInfo>,
}

/// GPU utilization and memory from nvidia-smi / rocm-smi.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuMetrics {
    pub name: String,
    pub utilization_percent: f32,
    pub vram_used_gb: f32,
    pub vram_total_gb: f32,
    pub temperature_celsius: Option<f32>,
}

/// A single process entry from ps / nvidia-smi pmon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub name: String,
    pub cpu_percent: f32,
    pub ram_mb: f32,
    pub gpu_percent: Option<f32>,
    pub vram_gb: Option<f32>,
    pub uptime_display: String,
}

impl SystemMetrics {
    pub fn cpu_bar_percent(&self) -> f32 {
        self.cpu_percent.clamp(0.0, 100.0)
    }

    pub fn ram_bar_percent(&self) -> f32 {
        if self.ram_total_gb > 0.0 {
            ((self.ram_used_gb / self.ram_total_gb) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        }
    }

    pub fn disk_bar_percent(&self) -> f32 {
        if self.disk_total_gb > 0.0 {
            ((self.disk_used_gb / self.disk_total_gb) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        }
    }
}

impl GpuMetrics {
    pub fn vram_bar_percent(&self) -> f32 {
        if self.vram_total_gb > 0.0 {
            ((self.vram_used_gb / self.vram_total_gb) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        }
    }
}

/// Severity tier for a resource metric value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceSeverity {
    Normal,
    Warning,
    Critical,
}

pub fn severity_for_percent(percent: f32) -> ResourceSeverity {
    if percent >= 85.0 {
        ResourceSeverity::Critical
    } else if percent >= 60.0 {
        ResourceSeverity::Warning
    } else {
        ResourceSeverity::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_percentages_clamp() {
        let metrics = SystemMetrics {
            cpu_percent: 150.0,
            ram_used_gb: 90.0,
            ram_total_gb: 80.0,
            disk_used_gb: 500.0,
            disk_total_gb: 1000.0,
            ..Default::default()
        };
        assert_eq!(metrics.cpu_bar_percent(), 100.0);
        assert_eq!(metrics.ram_bar_percent(), 100.0);
        assert_eq!(metrics.disk_bar_percent(), 50.0);
    }

    #[test]
    fn severity_thresholds() {
        assert_eq!(severity_for_percent(30.0), ResourceSeverity::Normal);
        assert_eq!(severity_for_percent(70.0), ResourceSeverity::Warning);
        assert_eq!(severity_for_percent(95.0), ResourceSeverity::Critical);
    }

    #[test]
    fn gpu_vram_percent() {
        let gpu = GpuMetrics {
            name: "A100".into(),
            utilization_percent: 97.0,
            vram_used_gb: 71.0,
            vram_total_gb: 80.0,
            temperature_celsius: Some(72.0),
        };
        let percent = gpu.vram_bar_percent();
        assert!(percent > 88.0 && percent < 89.0);
    }
}
