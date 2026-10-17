//! Live smoke test: every employee in `team.toml` must create a session bound
//! to its own OpenCode agent and reply to a trivial prompt.
//!
//! Requires a running `opencode service` and inference budget. Run explicitly:
//!
//!   cargo test --test live_employees -- --ignored --nocapture

use mywaker::opencode::OpenCode;
use mywaker::team::{Employee, Team};

#[ignore = "requires a running OpenCode service + inference budget"]
#[tokio::test]
async fn every_employee_binds_its_agent_and_replies() {
    let client = OpenCode::connect()
        .await
        .expect("connect to OpenCode service");
    let team = Team::load_or_starter();
    let directory = std::env::current_dir()
        .expect("current dir")
        .to_string_lossy()
        .to_string();

    assert!(
        !team.employees().is_empty(),
        "team.toml has no employees to test"
    );

    let mut failures = 0;
    for employee in team.employees() {
        match exercise_employee(&client, &directory, employee).await {
            Ok(reply) => {
                println!(
                    "PASS  {:<12} agent='{}' → \"{}\"",
                    employee.id,
                    employee.agent_id(),
                    truncate(&reply, 60)
                );
            }
            Err(err) => {
                failures += 1;
                println!(
                    "FAIL  {:<12} agent='{}': {err}",
                    employee.id,
                    employee.agent_id()
                );
            }
        }
    }

    assert_eq!(failures, 0, "{failures} employee(s) failed");
}

async fn exercise_employee(
    client: &OpenCode,
    directory: &str,
    employee: &Employee,
) -> Result<String, String> {
    let agent = employee.agent_id();
    let model = employee.model_ref();

    // The agent must actually bind (not fall back to the default agent): a
    // successful mywaker employee == a defined OpenCode agent.
    let (session, used_agent) = client
        .create_session_with_agent_fallback(directory, Some(agent), model.as_ref())
        .await
        .map_err(|e| format!("create_session: {e:#}"))?;
    if !used_agent {
        return Err(format!(
            "agent '{agent}' not defined (fell back to default)"
        ));
    }

    client
        .prompt(&session.id, "Reply with exactly: ok")
        .await
        .map_err(|e| format!("prompt: {e:#}"))?;

    read_reply(client, &session.id).await
}

/// Block until the session produces a complete assistant reply, then return its
/// text. `wait` is best-effort (its endpoint can drift); the transcript is the
/// source of truth.
async fn read_reply(client: &OpenCode, session_id: &str) -> Result<String, String> {
    let _ = client.wait(session_id).await;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let messages = client
            .messages(session_id)
            .await
            .map_err(|e| format!("messages: {e:#}"))?;

        if let Some(text) = messages
            .iter()
            .rev()
            .find(|m| m.role() == "assistant" && !m.is_incomplete())
            .map(|m| m.display_text())
        {
            let text = text.trim().to_string();
            if !text.is_empty() {
                return Ok(text);
            }
        }

        if std::time::Instant::now() > deadline {
            return Err("timed out waiting for a reply".to_string());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

fn truncate(text: &str, max: usize) -> String {
    let text = text.replace('\n', " ");
    if text.chars().count() <= max {
        return text;
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
