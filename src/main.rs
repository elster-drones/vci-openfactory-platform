// SPDX-License-Identifier: MIT
use std::{io, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};
use vci::{async_trait, Component, Config, MgmtError, Model, Result, State, VciError};

const DEADLINE: Duration = Duration::from_secs(30);
const OUTPUT_LIMIT: u64 = 1024 * 1024;

#[derive(Clone)]
struct Cli {
    program: PathBuf,
    deadline: Duration,
}

async fn capture(reader: impl AsyncRead + Unpin) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(OUTPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err(io::Error::other("CLI output exceeded 1 MiB"));
    }
    Ok(bytes)
}

impl Cli {
    // The CLI owns the schema, validation and rendering. No settings are cached.
    async fn call(&self, verb: &str, input: Option<&str>) -> Result<String> {
        let mut child = Command::new(&self.program)
            .args(["vci", verb])
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("missing stderr"))?;
        let execution = timeout(self.deadline, async {
            tokio::try_join!(
                async {
                    if let Some(mut pipe) = stdin {
                        pipe.write_all(input.unwrap_or_default().as_bytes()).await?;
                    }
                    Ok::<(), io::Error>(())
                },
                capture(stdout),
                capture(stderr),
                child.wait(),
            )
        })
        .await;
        let (_, stdout, stderr, status) = match execution {
            Ok(Ok(output)) => output,
            error => {
                // kill() waits and reaps too. Dropping an output/wait future alone
                // would leave the CLI running after the caller received its error.
                let _ = child.kill().await;
                return Err(VciError::Failed(match error {
                    Ok(Err(err)) => format!("openfactory-storage {verb}: {err}"),
                    _ => format!("openfactory-storage {verb} exceeded its deadline"),
                }));
            }
        };
        if status.success() {
            return String::from_utf8(stdout).map_err(|err| VciError::Failed(err.to_string()));
        }
        if let Ok(error) = serde_json::from_slice::<MgmtError>(&stdout) {
            // configd does not return a Set mgmterror to its caller. Failed is
            // visible in commit output; status --check confirms whether it applied.
            return Err(if verb == "set" {
                VciError::Failed(error.to_string())
            } else {
                error.into()
            });
        }
        Err(VciError::Failed(format!(
            "openfactory-storage {verb} exited {status}: {}",
            String::from_utf8_lossy(&stderr).trim()
        )))
    }
}

#[async_trait]
impl Config for Cli {
    async fn check(&self, input: &str) -> Result<()> {
        self.call("check", Some(input)).await.map(|_| ())
    }
    async fn set(&self, input: &str) -> Result<()> {
        self.call("set", Some(input)).await.map(|_| ())
    }
    async fn get(&self) -> Result<String> {
        self.call("get", None).await
    }
}

#[async_trait]
impl State for Cli {
    async fn get(&self) -> Result<String> {
        self.call("state", None).await
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli {
        program: "/usr/bin/openfactory-storage".into(),
        deadline: DEADLINE,
    };
    let model = Model::new("tech.openfactory.platform.v1")
        .config(cli.clone())
        .state(cli);
    let running = Component::new("tech.openfactory.platform", model)
        .run()
        .await?;
    if let Err(error) = running.wait().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn cli_forwarding_and_deadline() {
        let dir =
            Fixture(std::env::temp_dir().join(format!("platform-cli-test-{}", std::process::id())));
        fs::create_dir(&dir.0).unwrap();
        let cli = Cli {
            program: dir.0.join("cli"),
            deadline: Duration::from_millis(100),
        };
        let script = |body: &str| {
            fs::write(
                &cli.program,
                format!("#!/bin/sh\ncd '{}'\n{body}\n", dir.0.display()),
            )
            .unwrap();
            fs::set_permissions(&cli.program, fs::Permissions::from_mode(0o700)).unwrap();
        };
        script("printf '%s %s' \"$1\" \"$2\" > args; cat > input");
        cli.call("check", Some("{\"opaque\":true}")).await.unwrap();
        assert_eq!(fs::read_to_string(dir.0.join("args")).unwrap(), "vci check");
        assert_eq!(
            fs::read_to_string(dir.0.join("input")).unwrap(),
            "{\"opaque\":true}"
        );
        script("cat value");
        fs::write(dir.0.join("value"), "{\"value\":1}").unwrap();
        assert_eq!(cli.call("get", None).await.unwrap(), "{\"value\":1}");
        fs::write(dir.0.join("value"), "{\"value\":2}").unwrap();
        assert_eq!(cli.call("get", None).await.unwrap(), "{\"value\":2}");
        script("printf '%s' '{\"error-type\":\"application\",\"error-tag\":\"invalid-value\",\"error-severity\":\"error\",\"error-path\":\"/queue\",\"error-message\":\"synthetic refusal\"}'; exit 1");
        assert!(matches!(
            cli.call("check", Some("{}")).await,
            Err(VciError::Mgmt(_))
        ));
        assert!(
            matches!(cli.call("set", Some("{}")).await, Err(VciError::Failed(message)) if message.contains("synthetic refusal"))
        );
        for verb in ["check", "set", "get", "state"] {
            script("echo $$ > pid; exec sleep 300");
            assert!(
                matches!(cli.call(verb, Some("{}")).await, Err(VciError::Failed(message)) if message.contains("deadline"))
            );
            let pid = fs::read_to_string(dir.0.join("pid")).unwrap();
            assert!(
                !PathBuf::from(format!("/proc/{}", pid.trim())).exists(),
                "CLI was not reaped"
            );
        }
        script("head -c 1048577 /dev/zero; exec sleep 300");
        assert!(
            matches!(cli.call("state", None).await, Err(VciError::Failed(message)) if message.contains("exceeded 1 MiB"))
        );
    }
}
