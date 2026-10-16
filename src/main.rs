use clap::{Parser, Subcommand};
use color_eyre::eyre::{Result, eyre};
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
    /// Manage the employee roster.
    Employee {
        #[command(subcommand)]
        cmd: EmployeeCmd,
    },
    /// Discover / install skills from the agent-skills ecosystem (`npx skills`).
    Skills {
        #[command(subcommand)]
        cmd: SkillsCmd,
    },
}

#[derive(Subcommand)]
enum EmployeeCmd {
    /// List employees (with their skills).
    List,
    /// Add an employee.
    Add {
        id: String,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        role: Option<String>,
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Remove an employee.
    Rm { id: String },
    /// Update an employee's name/role/model.
    Set {
        id: String,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        role: Option<String>,
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Attach an installed skill to an employee.
    Skill { id: String, skill: String },
    /// Detach a skill from an employee.
    Unskill { id: String, skill: String },
}

#[derive(Subcommand)]
enum SkillsCmd {
    /// Search the ecosystem for skills (interactive).
    Find { query: Vec<String> },
    /// List installed skills.
    List,
    /// Install a skill package.
    Add {
        package: String,
        #[arg(short, long)]
        global: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Models) => return run_models().await,
        Some(Command::Route { goal }) => return run_route(&goal).await,
        Some(Command::Employee { cmd }) => return run_employee(cmd),
        Some(Command::Skills { cmd }) => return run_skills(cmd),
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
    let mut app = App::with_team(config, tx, mywaker::team::Team::load_or_starter());

    tui.enter()?;
    let result = app.run(&mut tui, events).await;
    tui.exit()?;

    result
}

/// `mywaker models`: fetch the live Kenari catalog and route each employee.
async fn run_models() -> Result<()> {
    use mywaker::team::Team;

    let team = Team::load_or_starter();
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

/// Manage the employee roster: list / add / rm / set / skill / unskill.
fn run_employee(cmd: EmployeeCmd) -> Result<()> {
    use mywaker::team::{Employee, Team};

    match cmd {
        EmployeeCmd::List => {
            let team = Team::load_or_starter();
            print_team(&team);
        }
        EmployeeCmd::Add {
            id,
            name,
            role,
            model,
        } => {
            let mut team = Team::load_or_starter();
            let mut employee = Employee::new(
                id.clone(),
                name.unwrap_or_else(|| id.clone()),
                role.unwrap_or_else(|| "general".to_string()),
            );
            employee.model = model;
            team.add_employee(employee).map_err(|e| eyre!("{e}"))?;
            team.save(&team_path()?)?;
            println!("added employee '{id}'");
        }
        EmployeeCmd::Rm { id } => {
            let mut team = Team::load_or_starter();
            if team.remove_employee(&id) {
                team.save(&team_path()?)?;
                println!("removed employee '{id}'");
            } else {
                return Err(eyre!("no employee '{id}'"));
            }
        }
        EmployeeCmd::Set {
            id,
            name,
            role,
            model,
        } => {
            let mut team = Team::load_or_starter();
            let employee = team
                .employee_mut(&id)
                .ok_or_else(|| eyre!("no employee '{id}'"))?;
            if let Some(name) = name {
                employee.name = name;
            }
            if let Some(role) = role {
                employee.role = role;
            }
            if model.is_some() {
                employee.model = model;
            }
            team.save(&team_path()?)?;
            println!("updated employee '{id}'");
        }
        EmployeeCmd::Skill { id, skill } => {
            let mut team = Team::load_or_starter();
            team.add_skill(&id, &skill).map_err(|e| eyre!("{e}"))?;
            team.save(&team_path()?)?;
            println!("attached skill '{skill}' to '{id}'");
        }
        EmployeeCmd::Unskill { id, skill } => {
            let mut team = Team::load_or_starter();
            team.remove_skill(&id, &skill).map_err(|e| eyre!("{e}"))?;
            team.save(&team_path()?)?;
            println!("detached skill '{skill}' from '{id}'");
        }
    }

    Ok(())
}

/// Discover / install skills via `npx skills`.
fn run_skills(cmd: SkillsCmd) -> Result<()> {
    use mywaker::skills;

    match cmd {
        SkillsCmd::Find { query } => skills::find(&query),
        SkillsCmd::List => skills::list(),
        SkillsCmd::Add { package, global } => skills::add(&package, global),
    }
}

/// Resolve the team file path (error if the config dir is unavailable).
fn team_path() -> Result<std::path::PathBuf> {
    mywaker::team::Team::default_path().ok_or_else(|| eyre!("could not determine config directory"))
}

fn print_team(team: &mywaker::team::Team) {
    if team.is_empty() {
        println!("(no employees — add one with `mywaker employee add <id>`)");
        return;
    }
    for employee in team.employees() {
        let skills = if employee.skills.is_empty() {
            "none".to_string()
        } else {
            employee.skills.join(", ")
        };
        println!(
            "{:<14} {} — {} — model: {} — skills: {}",
            employee.id,
            employee.name,
            employee.role,
            employee.model.as_deref().unwrap_or("default"),
            skills,
        );
    }
}
