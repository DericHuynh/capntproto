//! Give every protocol the same two physical cores, respecting the runner's cpuset.
use crate::Result;
use serde::Serialize;
use std::{collections::BTreeSet, fs, path::Path, process::Command};

#[derive(Debug, Serialize)]
pub(super) struct Placement {
    pub server_cpu: usize,
    pub client_cpu: usize,
    topology: String,
    allowed_cpus: String,
}

impl Placement {
    pub fn discover() -> Result<Self> {
        Self::parse(
            &fs::read_to_string("/proc/self/status")?,
            super::output("lscpu", &["-b", "-p=CPU,CORE,SOCKET"])?,
        )
    }

    fn parse(status: &str, topology: String) -> Result<Self> {
        let allowed_cpus = status
            .lines()
            .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))
            .ok_or("missing allowed CPU list")?
            .trim()
            .to_owned();
        let mut allowed = BTreeSet::new();
        for range in allowed_cpus.split(',') {
            let (start, end) = range.split_once('-').unwrap_or((range, range));
            let start: usize = start.parse()?;
            let end: usize = end.parse()?;
            if end < start || end > 1_048_576 {
                return Err("invalid allowed CPU range".into());
            }
            allowed.extend(start..=end);
        }
        let mut cores = BTreeSet::new();
        let mut selected = Vec::new();
        for row in topology
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
        {
            let fields: Vec<_> = row.split(',').collect();
            if fields.len() != 3 {
                return Err("invalid CPU topology".into());
            }
            let cpu: usize = fields[0].parse()?;
            let core: usize = fields[1].parse()?;
            let socket: usize = fields[2].parse()?;
            if allowed.contains(&cpu) && cores.insert((socket, core)) {
                selected.push(cpu);
            }
        }
        if selected.len() < 2 {
            return Err("comparison requires two allowed physical CPU cores".into());
        }
        Ok(Self {
            server_cpu: selected[0],
            client_cpu: selected[1],
            topology,
            allowed_cpus,
        })
    }

    pub fn command(exe: &Path, cpu: Option<usize>) -> Command {
        if let Some(cpu) = cpu {
            // taskset fails before exec if it cannot apply affinity. Children and
            // runtime threads inherit the singleton mask, including the C++ wrapper.
            let mut command = Command::new("taskset");
            command.args(["--cpu-list", &cpu.to_string()]).arg(exe);
            command
        } else {
            Command::new(exe)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn workers_inherit_exactly_the_selected_physical_core() {
        let placement = Placement::discover().unwrap();
        assert_ne!(placement.server_cpu, placement.client_cpu);
        for cpu in [None, Some(placement.server_cpu), Some(placement.client_cpu)] {
            let result = Placement::command(Path::new("cat"), cpu)
                .arg("/proc/self/status")
                .output()
                .unwrap();
            assert!(result.status.success());
            let status = String::from_utf8(result.stdout).unwrap();
            let allowed = status
                .lines()
                .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))
                .unwrap()
                .trim();
            assert_eq!(
                allowed,
                cpu.map_or_else(|| placement.allowed_cpus.clone(), |cpu| cpu.to_string())
            );
        }
    }

    #[test]
    fn placement_respects_cpuset_and_skips_smt_siblings() {
        let topology = "# CPU,Core,Socket\n0,0,0\n1,0,0\n2,1,0\n3,1,0\n4,0,1\n";
        let placement = Placement::parse("Cpus_allowed_list:\t1,3-4\n", topology.into()).unwrap();
        assert_eq!((placement.server_cpu, placement.client_cpu), (1, 3));
        let placement = Placement::parse("Cpus_allowed_list:\t0-1,4\n", topology.into()).unwrap();
        assert_eq!((placement.server_cpu, placement.client_cpu), (0, 4));
        assert!(Placement::parse("Cpus_allowed_list:\t0-1\n", topology.into()).is_err());
        assert!(Placement::parse("Cpus_allowed_list:\t3-1\n", topology.into()).is_err());
        assert!(Placement::parse("", topology.into()).is_err());
    }
}
