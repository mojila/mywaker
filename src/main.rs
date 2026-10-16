use clap::{Parser, Subcommand};
use color_eyre::eyre::Result;
use tokio::sync::mpsc;

use mywaker::app::{App, opencode_background};
use mywaker::config::Config;
use mywaker::event::EventHandler;
use mywaker::logging::init_logging;
use mywaker::message::Message;
use mywaker::tui::Tui;

#[derive(Parser)]
#[command(name = "mywaker")]
#[command(about = "Run a team of AI employees backed by OpenCode")]
struct Cli {
    /// Config file path
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Route each employee to the best model (live catalog + existing providers).
    Models,
    /// Auto-find the best cross-provider model for a task description.
    Route { goal: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Models) => return run_models().await,
        Some(Command::Route { goal }) => return run_route(&goal).await,
        None => {}
    }

    let config = Config::load(cli.config.as_deref())?;
    init_logging(&config, cli.debug)?;

    tracing::info!("starting mywaker");

    let (tx, rx) = mpsc::channel::<Message>(256);

    // Connect to OpenCode and forward live events in the background.
    tokio::spawn(opencode_background(tx.clone()));

    let mut tui = Tui::new()?;
    let events = EventHandler::new(config.tick_rate, rx);
    let mut app = App::new(config, tx);

    tui.enter()?;
    let result = app.run(&mut tui, events).await;
    tui.exit()?;

    result
}

/// `mywaker models`: fetch the live Kenari catalog and route each employee.
async fn run_models() -> Result<()> {
    use mywaker::team::Team;

    let team = Team::starter();
    let selections = mywaker::kenari::route_roster(team.employees()).await?;

    for selection in &selections {
        match &selection.choice {
            Some(choice) => {
                println!(
                    "{} ({}) → {} [{}] — {}",
                    selection.name,
                    selection.employee_id,
                    choice.id,
                    choice.provider,
                    choice.reason,
                );
            }
            None => {
                println!(
                    "{} ({}) → no suitable model",
                    selection.name, selection.employee_id,
                );
            }
        }
    }

    Ok(())
}

/// `mywaker route "<task>"`: infer role + complexity and pick the best model.
async fn run_route(goal: &str) -> Result<()> {
    use mywaker::kenari::{Role, TaskComplexity};

    let role = Role::from_goal(goal);
    let complexity = TaskComplexity::infer(goal);
    let choice = mywaker::kenari::auto_route(goal).await?;

    println!("task: {goal}");
    println!(
        "inferred: role={} complexity={}",
        role.label(),
        complexity.label()
    );
    match choice {
        Some(choice) => {
            println!(
                "model: {} [{}] — {}",
                choice.id, choice.provider, choice.reason
            );
        }
        None => println!("model: none"),
    }

    Ok(())
}
