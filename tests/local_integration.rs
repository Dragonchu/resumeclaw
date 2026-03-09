use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

struct TestDirGuard {
    path: PathBuf,
}

impl TestDirGuard {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirGuard {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is before UNIX epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("resumeclaw-{name}-{nanos}"))
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, content).expect("write file");
}

#[test]
fn cli_flow_can_run_with_mock_llm_script() {
    let root = TestDirGuard::new(unique_test_dir("local-integration"));
    let template_dir = root.path().join("template");
    let workspace_dir = root.path().join("workspace");
    let script_path = root.path().join("mock-llm.json");

    write_file(
        &template_dir.join("resume.tex"),
        r"\documentclass{article}
\begin{document}
Original Resume
\end{document}
",
    );
    write_file(
        &script_path,
        r#"[
  {
    "expect_last_user_message": "请把简历改成测试版本",
    "tool_calls": [
      {
        "id": "call-read",
        "name": "read_resume",
        "arguments": {}
      }
    ]
  },
  {
    "tool_calls": [
      {
        "id": "call-write",
        "name": "write_resume",
        "arguments": {
          "content": "\\documentclass{article}\n\\begin{document}\nUpdated by local integration test\n\\end{document}\n"
        }
      }
    ]
  },
  {
    "content": "本地集成测试已完成",
    "tool_calls": []
  }
]"#,
    );

    let exe = env!("CARGO_BIN_EXE_resumeclaw");
    let mut child = Command::new(exe)
        .current_dir(root.path())
        .env_remove("DISCORD_BOT_TOKEN")
        .env("LLM_PROVIDER", "mock")
        .env("LLM_MODEL", "mock-local")
        .env("MOCK_LLM_SCRIPT_PATH", &script_path)
        .env("RESUME_TEMPLATE_DIR", &template_dir)
        .env("WORKSPACE_DIR", &workspace_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn resumeclaw");

    {
        let stdin = child.stdin.as_mut().expect("child stdin");
        stdin
            .write_all("请把简历改成测试版本\n".as_bytes())
            .expect("write stdin");
    }

    let output = child.wait_with_output().expect("wait for process");
    assert!(
        output.status.success(),
        "process failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("本地集成测试已完成"),
        "stdout did not contain final response:\n{stdout}"
    );

    let resume =
        fs::read_to_string(workspace_dir.join("resume.tex")).expect("read workspace resume");
    assert!(
        resume.contains("Updated by local integration test"),
        "workspace resume was not updated:\n{resume}"
    );
}

#[test]
fn cargo_run_defaults_to_dev_mock_provider_and_example_template() {
    let root = TestDirGuard::new(unique_test_dir("zero-config"));
    let workspace_dir = root.path().join("workspace");
    fs::create_dir_all(root.path()).expect("create test root");

    let exe = env!("CARGO_BIN_EXE_resumeclaw");
    let mut child = Command::new(exe)
        .current_dir(root.path())
        .env_remove("DISCORD_BOT_TOKEN")
        .env_remove("LLM_PROVIDER")
        .env_remove("LLM_MODEL")
        .env_remove("MOCK_LLM_SCRIPT_PATH")
        .env_remove("RESUME_TEMPLATE_DIR")
        .env("WORKSPACE_DIR", &workspace_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn resumeclaw");

    // The first line consumes the bundled scripted example, and the second line
    // verifies that zero-config dev mode keeps responding like a REPL instead
    // of surfacing "mock script exhausted" errors.
    let dev_mode_input = "进入开发模式\n第二条消息\n";
    {
        let stdin = child.stdin.as_mut().expect("child stdin");
        stdin
            .write_all(dev_mode_input.as_bytes())
            .expect("write stdin");
    }

    let output = child.wait_with_output().expect("wait for process");
    assert!(
        output.status.success(),
        "process failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("开发模式已启用"),
        "stdout did not contain bundled mock response:\n{stdout}"
    );
    assert!(
        stdout.contains("已收到你的消息：第二条消息"),
        "stdout did not contain the dev REPL fallback response:\n{stdout}"
    );
    assert!(
        !stdout.contains("mock script exhausted before conversation completed"),
        "stdout still exposed mock exhaustion:\n{stdout}"
    );

    let resume =
        fs::read_to_string(workspace_dir.join("resume.tex")).expect("read workspace resume");
    assert!(
        resume.contains("Dev Example Resume"),
        "workspace resume did not come from bundled dev template:\n{resume}"
    );
}

#[test]
fn cargo_run_dev_mode_supports_listing_and_direct_tool_calls() {
    let root = TestDirGuard::new(unique_test_dir("dev-cli-tools"));
    let workspace_dir = root.path().join("workspace");
    fs::create_dir_all(root.path()).expect("create test root");

    let exe = env!("CARGO_BIN_EXE_resumeclaw");
    let mut child = Command::new(exe)
        .current_dir(root.path())
        .env_remove("DISCORD_BOT_TOKEN")
        .env_remove("LLM_PROVIDER")
        .env_remove("LLM_MODEL")
        .env_remove("MOCK_LLM_SCRIPT_PATH")
        .env_remove("RESUME_TEMPLATE_DIR")
        .env("WORKSPACE_DIR", &workspace_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn resumeclaw");

    {
        let stdin = child.stdin.as_mut().expect("child stdin");
        stdin
            .write_all("/list\n/read_resume\n".as_bytes())
            .expect("write stdin");
    }

    let output = child.wait_with_output().expect("wait for process");
    assert!(
        output.status.success(),
        "process failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("/read_resume"),
        "stdout did not contain direct tool list output:\n{stdout}"
    );
    assert!(
        stdout.contains("/write_resume"),
        "stdout did not contain write tool usage:\n{stdout}"
    );
    assert!(
        stdout.contains("直接调用 /read_resume 的结果"),
        "stdout did not contain direct tool execution output:\n{stdout}"
    );
    assert!(
        stdout.contains("Dev Example Resume"),
        "stdout did not contain resume content from direct tool execution:\n{stdout}"
    );
}

#[test]
fn cargo_run_dev_mode_supports_multiline_write_resume() {
    let root = TestDirGuard::new(unique_test_dir("dev-cli-multiline-write"));
    let workspace_dir = root.path().join("workspace");
    fs::create_dir_all(&workspace_dir).expect("create workspace dir");

    let exe = env!("CARGO_BIN_EXE_resumeclaw");
    let mut child = Command::new(exe)
        .current_dir(root.path())
        .env_remove("DISCORD_BOT_TOKEN")
        .env_remove("LLM_PROVIDER")
        .env_remove("LLM_MODEL")
        .env_remove("MOCK_LLM_SCRIPT_PATH")
        .env_remove("RESUME_TEMPLATE_DIR")
        .env("WORKSPACE_DIR", &workspace_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn resumeclaw");

    {
        let stdin = child.stdin.as_mut().expect("child stdin");
        stdin
            .write_all(
                b"/write_resume\n\\documentclass{article}\n\\begin{document}\nLine One\n\nLine Two\n\\end{document}\n/end\n/read_resume\n",
            )
            .expect("write stdin");
    }

    let output = child.wait_with_output().expect("wait for process");
    assert!(
        output.status.success(),
        "process failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("进入 /write_resume 多行输入模式"),
        "stdout did not show multiline mode prompt:\n{stdout}"
    );
    assert!(
        stdout.contains("直接调用 /write_resume 的结果"),
        "stdout did not contain multiline direct tool result:\n{stdout}"
    );
    assert!(
        stdout.contains("Successfully wrote"),
        "stdout did not contain write success output:\n{stdout}"
    );

    let resume =
        fs::read_to_string(workspace_dir.join("resume.tex")).expect("read workspace resume");
    assert!(
        resume.contains("Line One\n\nLine Two"),
        "workspace resume did not preserve multiline content:\n{resume}"
    );
}
