// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    fmt::{Debug, Display},
    path::PathBuf,
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use sui_swarm_config::genesis_config::GenesisConfig;
use sui_types::multiaddr::Multiaddr;

use crate::{
    benchmark::{BenchmarkParameters, BenchmarkType},
    client::Instance,
    settings::Settings,
};

use super::{ProtocolCommands, ProtocolMetrics};

// todo: make configurable
const NUM_WORKERS: usize = 1;
const BASE_PORT: usize = 5000;

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NarwhalBenchmarkType {
    /// The size of each transaciton in bytes
    size: usize,
}

impl Debug for NarwhalBenchmarkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.size)
    }
}

impl Display for NarwhalBenchmarkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tx size {}b", self.size)
    }
}

impl FromStr for NarwhalBenchmarkType {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            size: s.parse::<usize>()?.min(1),
        })
    }
}

impl BenchmarkType for NarwhalBenchmarkType {}

/// All configurations information to run a narwhal client or validator.
pub struct NarwhalProtocol {
    working_dir: PathBuf,
}

impl ProtocolCommands<NarwhalBenchmarkType> for NarwhalProtocol {
    fn protocol_dependencies(&self) -> Vec<&'static str> {
        // todo: remove?
        vec![
            // Install typical sui dependencies.
            "sudo apt-get -y install curl git-all clang cmake gcc libssl-dev pkg-config libclang-dev",
            // This dependency is missing from the Sui docs.
            "sudo apt-get -y install libpq-dev",
        ]
    }

    fn db_directories(&self) -> Vec<PathBuf> {
        let consensus_db = [&self.working_dir, &format!("db-*").into()]
            .iter()
            .collect();
        vec![consensus_db]
    }

    fn genesis_command<'a, I>(&self, instances: I) -> String
    where
        I: Iterator<Item = &'a Instance>,
    {
        let working_dir = self.working_dir.display();
        let ips = instances
            .map(|x| x.main_ip.to_string())
            .collect::<Vec<_>>()
            .join(" ");

        let genesis = [
            "cargo run --release --bin narwhal-node benchmark-genesis",
            &format!(
                " --working-directory {working_dir} --ips {ips} --num-workers {NUM_WORKERS} --base-port {BASE_PORT}"
            ),
        ]
        .join(" ");

        [
            &format!("mkdir -p {working_dir}"),
            "source $HOME/.cargo/env",
            &genesis,
        ]
        .join(" && ")
    }

    fn monitor_command<I>(&self, _instances: I) -> Vec<(Instance, String)>
    where
        I: IntoIterator<Item = Instance>,
    {
        // instances
        //     .into_iter()
        //     .map(|i| {
        //         (
        //             i,
        //             "tail -f --pid=$(pidof sui) -f /dev/null; tail -100 node.log".to_string(),
        //         )
        //     })
        //     .collect()
        vec![]
    }

    fn node_command<I>(
        &self,
        instances: I,
        _parameters: &BenchmarkParameters<NarwhalBenchmarkType>,
    ) -> Vec<(Instance, String)>
    where
        I: IntoIterator<Item = Instance>,
    {
        let working_dir = self.working_dir.clone();

        instances
            .into_iter()
            .enumerate()
            .map(|(i, instance)| {
                let primary_keys: PathBuf = [&working_dir, &format!("primary-{i}-key.json").into()]
                    .iter()
                    .collect();
                let primary_network_keys: PathBuf = [
                    &working_dir,
                    &format!("primary-{i}-network-key.json").into(),
                ]
                .iter()
                .collect();
                // todo: fix this work for multiple workers
                let worker_keys: PathBuf = [&working_dir, &format!("worker-{i}-key.json").into()]
                    .iter()
                    .collect();
                let committee: PathBuf = [&working_dir, &format!("committee.json").into()]
                    .iter()
                    .collect();
                let workers: PathBuf = [&working_dir, &format!("workers.json").into()]
                    .iter()
                    .collect();
                let store: PathBuf = [&working_dir, &format!("db-{i}").into()].iter().collect();
                let parameters: PathBuf = [&working_dir, &format!("parameters.json").into()]
                    .iter()
                    .collect();

                let run = [
                    "cargo run --release --bin narwhal-node run ",
                    &format!(
                        "--primary-keys {} --primary-network-keys {} ",
                        primary_keys.display(),
                        primary_network_keys.display()
                    ),
                    &format!(
                        "--worker-keys {} --committee {} --workers {} ",
                        worker_keys.display(),
                        committee.display(),
                        workers.display()
                    ),
                    &format!(
                        "--store {} --parameters {} authority 0",
                        store.display(),
                        parameters.display()
                    ),
                ]
                .join(" ");
                let command = ["source $HOME/.cargo/env", &run].join(" && ");

                (instance, command)
            })
            .collect()
    }

    fn client_command<I>(
        &self,
        instances: I,
        parameters: &BenchmarkParameters<NarwhalBenchmarkType>,
    ) -> Vec<(Instance, String)>
    where
        I: IntoIterator<Item = Instance>,
    {
        let clients: Vec<_> = instances.into_iter().collect();
        // 2 ports used per authority so add 2 * num authorities to base port
        let mut worker_base_port = BASE_PORT + (2 * clients.len());

        // RUST_LOG=info cargo run --release --features benchmark --bin narwhal-benchmark-client http://0.0.0.0:5010 512 100000 http://0.0.0.0:5012,http://0.0.0.0:5014,http://0.0.0.0:5008

        let transaction_addresses: Vec<_> = clients
            .iter()
            .map(|instance| {
                let transaction_address = format!("{}:{}", instance.main_ip, worker_base_port);
                worker_base_port += 2;
                transaction_address
            })
            .collect();

        clients
            .into_iter()
            .enumerate()
            .map(|(i, instance)| {
                let run = [
                    "cargo run --release --features benchmark --bin narwhal-benchmark-client ",
                    &format!(
                        "{} {} {} {}",
                        transaction_addresses[i],
                        parameters.benchmark_type.size,
                        parameters.load,
                        transaction_addresses.join(",")
                    ),
                ]
                .join(" ");
                let command = ["source $HOME/.cargo/env", &run].join(" && ");

                (instance, command)
            })
            .collect()
    }
}

impl NarwhalProtocol {
    const CLIENT_METRICS_PORT: u16 = GenesisConfig::BENCHMARKS_PORT_OFFSET + 2000;

    /// Make a new instance of the Narwhal protocol commands generator.
    pub fn new(settings: &Settings) -> Self {
        Self {
            working_dir: [&settings.working_dir, &sui_config::SUI_CONFIG_DIR.into()]
                .iter()
                .collect(),
        }
    }

    /// Creates the network addresses in multi address format for the instances. It returns the
    /// Instance and the corresponding address.
    pub fn resolve_network_addresses(
        instances: impl IntoIterator<Item = Instance>,
    ) -> Vec<(Instance, Multiaddr)> {
        let instances: Vec<Instance> = instances.into_iter().collect();
        let ips: Vec<_> = instances.iter().map(|x| x.main_ip.to_string()).collect();
        let genesis_config = GenesisConfig::new_for_benchmarks(&ips);
        let mut addresses = Vec::new();
        if let Some(validator_configs) = genesis_config.validator_config_info.as_ref() {
            for (i, validator_info) in validator_configs.iter().enumerate() {
                let address = &validator_info.network_address;
                addresses.push((instances[i].clone(), address.clone()));
            }
        }
        addresses
    }
}

impl ProtocolMetrics for NarwhalProtocol {
    const BENCHMARK_DURATION: &'static str = "benchmark_duration";
    const TOTAL_TRANSACTIONS: &'static str = "latency_s_count";
    const LATENCY_BUCKETS: &'static str = "latency_s";
    const LATENCY_SUM: &'static str = "latency_s_sum";
    const LATENCY_SQUARED_SUM: &'static str = "latency_squared_s";

    fn nodes_metrics_path<I>(&self, _instances: I) -> Vec<(Instance, String)>
    where
        I: IntoIterator<Item = Instance>,
    {
        // let (ips, instances): (Vec<_>, Vec<_>) = instances
        //     .into_iter()
        //     .map(|x| (x.main_ip.to_string(), x))
        //     .unzip();

        // GenesisConfig::new_for_benchmarks(&ips)
        //     .validator_config_info
        //     .expect("No validator in genesis")
        //     .iter()
        //     .zip(instances)
        //     .map(|(config, instance)| {
        //         let path = format!(
        //             "{}:{}{}",
        //             instance.main_ip,
        //             config.metrics_address.port(),
        //             mysten_metrics::METRICS_ROUTE
        //         );
        //         (instance, path)
        //     })
        //     .collect()
        Vec::new()
    }

    fn clients_metrics_path<I>(&self, instances: I) -> Vec<(Instance, String)>
    where
        I: IntoIterator<Item = Instance>,
    {
        instances
            .into_iter()
            .map(|instance| {
                let path = format!(
                    "{}:{}{}",
                    instance.main_ip,
                    Self::CLIENT_METRICS_PORT,
                    mysten_metrics::METRICS_ROUTE
                );
                (instance, path)
            })
            .collect()
    }
}
