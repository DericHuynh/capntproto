use anyhow::{ensure, Result};
use clap::Subcommand;
use std::path::PathBuf;
#[derive(Subcommand)]
pub enum Args {
    Generate {
        #[arg(value_parser=["network","features","realtime","all"])]
        family: String,
        #[arg(long, default_value = "verification/configs")]
        output: PathBuf,
    },
}
pub fn run(a: Args) -> Result<()> {
    match a {
        Args::Generate { family, output } => {
            let data: serde_json::Value =
                serde_json::from_str(include_str!("../data/models.json"))?;
            for (name, rows) in data.as_object().unwrap() {
                if family != "all" && *name != family {
                    continue;
                }
                for row in rows.as_array().unwrap() {
                    let name = crate::string(row, "name")?;
                    ensure!(
                        name.chars().all(|c| c.is_ascii_alphanumeric()),
                        "invalid config name"
                    );
                    let mut text = format!("\\* @module: {}\n", crate::string(row, "module")?);
                    if let Some(v) = row["violation"].as_str() {
                        text += &format!("\\* @violation: {v}\n");
                    }
                    text += &format!("SPECIFICATION {}\nCONSTANTS\n", crate::string(row, "spec")?);
                    for line in row["constants"].as_array().unwrap() {
                        text += &format!(" {}\n", line.as_str().unwrap());
                    }
                    text += "CHECK_DEADLOCK FALSE\nINVARIANTS\n";
                    for line in row["invariants"].as_array().unwrap() {
                        text += &format!(" {}\n", line.as_str().unwrap());
                    }
                    if !row["properties"].as_array().unwrap().is_empty() {
                        text += "PROPERTIES\n";
                        for line in row["properties"].as_array().unwrap() {
                            text += &format!(" {}\n", line.as_str().unwrap());
                        }
                    }
                    crate::write(output.join(format!("{name}.cfg")), text)?;
                }
            }
            Ok(())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_configs_match_checked_in_models() {
        let dir = tempfile::tempdir().unwrap();
        run(Args::Generate {
            family: "all".into(),
            output: dir.path().to_owned(),
        })
        .unwrap();
        let mut count = 0;
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let entry = entry.unwrap();
            let path = crate::root()
                .join("verification/configs")
                .join(entry.file_name());
            assert_eq!(
                std::fs::read(entry.path()).unwrap(),
                std::fs::read(&path).unwrap(),
                "{}",
                path.display()
            );
            count += 1;
        }
        assert!(count > 30);
    }
}
