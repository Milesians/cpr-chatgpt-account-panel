//! 测票与 ModelTrace、自动探测、通知和分组。

mod admin;
pub mod bank_sync;
pub mod codex;
pub mod config;
pub mod fingerprint;
pub mod groups;
pub mod health;
mod host;
pub(crate) mod management;
pub mod modeltrace;
pub mod runner;
pub mod scheduler;
pub mod series;
pub mod store;
pub mod telegram;
pub mod turnstate;
pub mod verdict;

use gateway_plugin_sdk::{PluginFault, client::HostClient};

pub(crate) struct Probe {
    runner: runner::Runner,
    scheduler: scheduler::Scheduler,
    groups: groups::Groups,
}

impl Probe {
    pub(crate) fn new(config: config::Config) -> Self {
        Self {
            runner: runner::Runner::new(config),
            scheduler: scheduler::Scheduler::default(),
            groups: groups::Groups::default(),
        }
    }

    pub(crate) async fn reconcile(&self, host: &HostClient) -> Result<(), PluginFault> {
        if self.runner.config.probe_maintenance_enabled {
            self.scheduler
                .reconcile(&self.runner, &self.groups, host)
                .await?;
        }
        Ok(())
    }
}
