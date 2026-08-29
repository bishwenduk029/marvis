use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

pub fn run(user_text: &str, mut on_activity: impl FnMut(&str)) -> Result<String, String> {
    let provider = std::env::var("MARVIS_LLM_PROVIDER").unwrap_or_else(|_| "openrouter".to_string());
    let model = std::env::var("MARVIS_LLM_MODEL").unwrap_or_else(|_| "z-ai/glm-5.3-flash".to_string());

    let mut child = spawn_zerostack(&provider, &model, user_text)?;

    let mut reply = String::new();
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines() {
            let line = line.map_err(|e| e.to_string())?;
            on_activity(line.trim());
            reply.push_str(&line);
            reply.push('\n');
        }
    }
    let output = child.wait().map_err(|e| e.to_string())?;
    if !output.success() {
        // stderr goes to our stderr since we didn't pipe it; report exit status
        return Err(format!("zerostack exited with {output}"));
    }
    Ok(reply.trim().to_string())
}

fn spawn_zerostack(provider: &str, model: &str, text: &str) -> Result<std::process::Child, String> {
    let bin = std::env::var("MARVIS_ZEROSTACK_BIN").unwrap_or_else(|_| "zerostack".to_string());
    Command::new(&bin)
        .arg("-p")
        .arg("--pure-stdout")
        .arg("--provider")
        .arg(provider)
        .arg("--model")
        .arg(model)
        .arg("--no-session")
        .arg(text)
        .current_dir(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn zerostack: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_fake_zerostack_script() {
        let dir = std::env::temp_dir().join(format!("marvis-zs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("fake-zs.sh");
        std::fs::write(&script, "#!/bin/sh\necho 'line one'\necho 'line two'\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        std::env::set_var("MARVIS_ZEROSTACK_BIN", &script);
        std::env::set_var("MARVIS_LLM_PROVIDER", "test");
        std::env::set_var("MARVIS_LLM_MODEL", "test-model");

        let mut fired = 0;
        let reply = run("hi", |_| fired += 1).unwrap();
        assert_eq!(reply, "line one\nline two");
        assert!(fired >= 2);

        std::fs::remove_dir_all(&dir).ok();
    }
}
