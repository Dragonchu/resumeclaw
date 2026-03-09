use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
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
    let root = unique_test_dir("local-integration");
    let template_dir = root.join("template");
    let workspace_dir = root.join("workspace");
    let script_path = root.join("mock-llm.json");

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

    let _ = fs::remove_dir_all(root);
}
